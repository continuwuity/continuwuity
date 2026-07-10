use std::{borrow::Borrow, collections::HashMap, sync::Arc, time::Instant};

use conduwuit::{
	Err, Result, debug, debug_error, debug_info, err, info, is_equal_to, is_true,
	matrix::{Event, EventTypeExt, PduEvent, StateKey, state_res},
	result::DebugInspect,
	trace,
	utils::{
		IterStream,
		mutex_map::Guard,
		stream::{BroadbandExt, ReadyExt},
	},
};
use futures::{FutureExt, StreamExt, future::ready};
use ruma::{
	CanonicalJsonObject, OwnedEventId, OwnedRoomId, RoomId, ServerName, api::error::ErrorKind,
	events::StateEventType, room_version_rules::RoomVersionRules,
};
use tokio::join;

use super::get_room_version_rules;
use crate::rooms::{
	state_compressor::{CompressedState, HashSetCompressStateEvent},
	timeline::RawPduId,
};

impl super::Service {
	#[tracing::instrument(name="upgrade_outlier", skip_all, fields(event_id=%incoming_pdu.event_id()))]
	pub(super) async fn upgrade_outlier_to_timeline_pdu(
		&self,
		incoming_pdu: PduEvent,
		mut val: CanonicalJsonObject,
		create_event: &PduEvent,
		origin: &ServerName,
		room_id: &RoomId,
	) -> Result<Option<RawPduId>> {
		let (pduid, rejected, soft_failed) = join!(
			self.services.timeline.get_pdu_id(incoming_pdu.event_id()),
			self.services
				.pdu_metadata
				.is_event_rejected(incoming_pdu.event_id()),
			self.services
				.pdu_metadata
				.is_event_soft_failed(incoming_pdu.event_id())
		);
		if let Ok(id) = pduid {
			trace!(event_id=%incoming_pdu.event_id(), "Skipping upgrade of already upgraded PDU");
			return Ok(Some(id));
		} else if rejected {
			return Err!(Request(Forbidden("Event has been rejected")));
		} else if soft_failed {
			// Soft-failed events cannot be promoted.
			return Err!(Request(Forbidden("Event has been soft-failed")));
		}

		// These should never happen, but they're good last-minute sanity checks to
		// ensure we never promote totally illegal events.
		assert_eq!(
			*create_event.kind(),
			StateEventType::RoomCreate.into(),
			"tried to upgrade a PDU with a create_event that is not a room create event"
		);
		assert_eq!(
			incoming_pdu.room_id_or_hash(),
			*room_id,
			"room ID mismatch: PDU room ID differs from parameter"
		);

		debug!(
			event_id = %incoming_pdu.event_id,
			"Upgrading PDU from outlier to timeline"
		);
		let timer = Instant::now();
		let min_depth = self.services.metadata.get_mindepth(room_id).await;
		let room_version_rules = get_room_version_rules(create_event)?;

		// We now need to resolve the state before the event so that we can perform PDU
		// check 5 (event auth passes based on state before the event). To do this, we
		// either need to have all the prev events locally, or ask a remote server
		// for the state at the event.
		let (passes_state_before, state_before) = self
			.is_authorised_by_state_before(
				&incoming_pdu,
				&room_version_rules,
				create_event,
				origin,
			)
			.await?;

		if !passes_state_before {
			self.reject_and_persist(incoming_pdu.event_id(), &val);
			return Err!(Request(Forbidden(
				"Event authorisation fails based on the state before the event"
			)));
		}

		// Now that we know the event passes both self-authentication, and
		// authentication based on the state before the event, we need to check that it
		// passes based on the *current* room state (state across all forward
		// extremities). If it doesn't, we accept it, but soft-fail it, and this
		// prevents it being promoted.

		// We lock the room here to prevent the current state from changing beneath us
		// mid-check.
		trace!(
			room_id = %room_id,
			"Locking the room"
		);
		let state_lock = self.services.state.mutex.lock(room_id).await;
		let passes_current_state = self
			.is_authorised_by_current_state(&incoming_pdu, &room_version_rules, create_event)
			.await?;

		// Determine whether this PDU should be soft-failed.
		// If the auth check failed, invariably yes. Otherwise, only if the user isn't
		// allowed to redact the target event (if any).
		let mut should_soft_fail =
			match (passes_current_state, incoming_pdu.redacts_id(&room_version_rules)) {
				| (false, _) => true,
				| (true, None) => false,
				| (true, Some(redact_id)) => self
					.services
					.state_accessor
					.user_can_redact(&redact_id, incoming_pdu.sender(), room_id, true)
					.await
					.is_ok_and(is_true!()),
			};

		if !should_soft_fail {
			// Now we can perform check 7, which is ensuring the event passes policy server
			// checks.
			// We explicitly only do this if we aren't already going to soft-fail the event,
			// since the policy server refusing this event also soft-fails it.
			debug!(event_id = %incoming_pdu.event_id, "Checking policy server for event");
			should_soft_fail = self
				.policy_server_check(&incoming_pdu, &mut val, &room_version_rules)
				.await?;

			// TODO: this is supposed to hide redactions from policy servers and janitorial
			// bots, however, for full efficacy it also needs to hide redactions for
			// unknown events. This needs to be investigated at a later time.
			if let Some(redact_id) = incoming_pdu.redacts_id(&room_version_rules) {
				debug!(
					redact_id = %redact_id,
					"Checking if redaction is for a soft-failed/rejected event"
				);
				if !self
					.services
					.pdu_metadata
					.is_event_accepted(&redact_id)
					.await
				{
					debug_info!(
						"Soft-failing valid redaction because it targets a non-accepted event"
					);
					should_soft_fail = true;
				}
			}
		}

		// The PDU has now passed all checks! We can now promote it (or soft-fail it if
		// the verdict is such).
		trace!("Appending pdu to timeline");
		let mut extremities: Vec<_> = self
			.services
			.state
			.get_forward_extremities(room_id)
			.collect()
			.await;
		if !should_soft_fail {
			// Per https://spec.matrix.org/unstable/server-server-api/#soft-failure, soft-failed events
			// are not added as forward extremities.
			// This also means we set the state here.
			// We do this BEFORE setting the extremities so that there's never a point in
			// time where we have fresh extremities referencing stale state.
			extremities = self
				.progress_state_and_extremities(
					&incoming_pdu,
					&room_version_rules,
					state_before.clone(),
					extremities,
					&state_lock,
				)
				.await?;
		}

		let state_ids_compressed: Arc<CompressedState> = self
			.services
			.state_compressor
			.compress_state_events(state_before.iter().map(|(ssk, eid)| (ssk, eid.borrow())))
			.collect()
			.map(Arc::new)
			.await;
		let pdu_id = self
			.services
			.timeline
			.append_incoming_pdu(
				&incoming_pdu,
				val,
				extremities.iter().map(Borrow::borrow),
				state_ids_compressed,
				should_soft_fail,
				&state_lock,
				room_id,
			)
			.await?;

		if should_soft_fail {
			self.services
				.pdu_metadata
				.mark_event_soft_failed(incoming_pdu.event_id());

			debug_info!(
				elapsed = ?timer.elapsed(),
				event_id = %incoming_pdu.event_id,
				"Event was soft failed"
			);
		} else {
			debug_info!(
				elapsed = ?timer.elapsed(),
				"Accepted",
			);
		}

		// Event has passed all auth/stateres checks
		drop(state_lock);
		if incoming_pdu.depth > min_depth && incoming_pdu.state_key().is_some() {
			self.services
				.metadata
				.set_mindepth(room_id, incoming_pdu.depth.into());
			trace!("Increased room's min depth from {} to {}", min_depth, incoming_pdu.depth);
		}

		Ok(pdu_id)
	}

	/// Checks that the event passes PDU check 5, which ensures that the event
	/// is authorised based on the state before the event (which is the resolved
	/// state across all prev events).
	///
	/// Returns a boolean indicating whether the event is authorised, and also
	/// the resolved state before the event for later use. Returns an error if
	/// state fetching or auth checking fails.
	async fn is_authorised_by_state_before(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		create_event: &PduEvent,
		origin: &ServerName,
	) -> Result<(bool, HashMap<u64, OwnedEventId>)> {
		debug!(
			event_id = %incoming_pdu.event_id,
			"Resolving state at event"
		);
		let room_id = incoming_pdu.room_id_or_hash();

		// If the incoming event only has one prev event, we can just use the state at
		// that event, but otherwise we have to resolve across each fork. If we're
		// missing even one of the prev events, we have to ask a remote server for help.
		//
		// TODO: this can be optimised by only loading auth chain events into memory,
		// rather than the entire state.
		let state_before = if incoming_pdu.prev_events().count() == 1 {
			self.state_at_incoming_degree_one(&incoming_pdu).await?
		} else {
			self.state_at_incoming_resolved(&incoming_pdu, &room_id, room_version_rules)
				.await?
		};
		let state_before = match state_before {
			| Some(s) => s,
			| None => {
				trace!("Could not calculate incoming state, asking remote {origin} for it");
				self.fetch_state(origin, create_event, &room_id, incoming_pdu.event_id())
					.await
					.debug_inspect_err(|e| {
						debug_error!("Could not fetch state from {origin}: {e}");
					})?
			},
		};

		if state_before.is_empty()
			&& *incoming_pdu.event_type() != StateEventType::RoomCreate.into()
		{
			// This can happen if the remote sends an event but cannot be reached to fetch
			// the state at it, and all other servers in the room (which might just be the
			// unreachable server) are unable to provide required info.
			// returning an error here allows the upgrade to be attempted at another time.
			return Err!(Request(Forbidden("Could not resolve incoming state before event")));
		}
		trace!(state_events = state_before.len(), "Calculated incoming state");

		let state_fetch_state = &state_before;
		let state_fetch = |k: StateEventType, s: StateKey| async move {
			let shortstatekey = self.services.short.get_shortstatekey(&k, &s).await.ok()?;

			let event_id = state_fetch_state.get(&shortstatekey)?;
			self.services.timeline.get_pdu(event_id).await.ok()
		};

		debug!(
			event_id = %incoming_pdu.event_id,
			"Running state-before auth check"
		);

		// PDU check: 5
		let auth_check = state_res::event_auth::auth_check(
			room_version_rules,
			incoming_pdu,
			None, // TODO: third party invite
			|ty, sk| state_fetch(ty.clone(), sk.into()),
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;
		Ok((auth_check, state_before))
	}

	/// Checks that the event passes PDU check 6, which ensures that the event
	/// is authorised based on the room's current state (which is the resolved
	/// state across all current forward extremities).
	///
	/// Returns a boolean indicating whether the event is authorised, or an
	/// error if the auth check fails.
	async fn is_authorised_by_current_state(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		create_event: &PduEvent,
	) -> Result<bool> {
		debug!(
			event_id = %incoming_pdu.event_id,
			"Gathering auth events"
		);
		let auth_events = self
			.services
			.state
			.get_auth_events(
				&incoming_pdu.room_id_or_hash(),
				incoming_pdu.kind(),
				incoming_pdu.sender(),
				incoming_pdu.state_key(),
				incoming_pdu.content(),
				room_version_rules,
			)
			.await?;

		let state_fetch = |k: &StateEventType, s: &str| {
			let key = k.with_state_key(s);
			ready(auth_events.get(&key).map(ToOwned::to_owned))
		};

		debug!(
			event_id = %incoming_pdu.event_id,
			"Running current state auth check"
		);
		state_res::event_auth::auth_check(
			room_version_rules,
			incoming_pdu,
			None, // third-party invite
			state_fetch,
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))
	}

	/// Performs PDU check 7 - does the policy server allow this event.
	///
	/// If the policy server forbids the event, false is returned. If there is a
	/// problem contacting the policy server, or it returns an unrecognised
	/// response, an appropriate error is returned.
	async fn policy_server_check(
		&self,
		incoming_pdu: &PduEvent,
		pdu_json: &mut CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result<bool> {
		let event_id = pdu_json
			.remove("event_id")
			.expect("event_id should be present in pdu_json at this stage");
		if let Err(e) = self
			.policy_server_allows_event(
				incoming_pdu,
				pdu_json,
				&incoming_pdu.room_id_or_hash(),
				room_version_rules,
				true,
			)
			.await
			.debug_inspect(|()| {
				debug!(
					event_id = %incoming_pdu.event_id,
					"Event has passed policy server check."
				);
			}) {
			return if matches!(e.kind(), ErrorKind::Forbidden) {
				info!(
					event_id = %incoming_pdu.event_id,
					error = %e,
					"Event has been marked as spam by policy server: {}",
					e.message(),
				);
				Ok(false)
			} else {
				Err(e)
			};
		}
		pdu_json.insert("event_id".to_owned(), event_id);
		Ok(true)
	}

	/// Derives new room state from the incoming event and filters forward
	/// extremities accordingly. Does not set forward extremities.
	///
	/// Only call this function if the incoming PDU is not soft-failed or
	/// rejected.
	async fn progress_state_and_extremities(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		state_before: HashMap<u64, OwnedEventId>,
		forward_extremities: Vec<OwnedEventId>,
		state_lock: &Guard<OwnedRoomId, ()>,
	) -> Result<Vec<OwnedEventId>> {
		if incoming_pdu.state_key().is_some() {
			debug!("Event is a state-event. Deriving new room state");
			self.derive_new_state(incoming_pdu, room_version_rules, state_before, state_lock)
				.await?;
		}

		// Now we calculate the set of extremities this room has after the incoming
		// event has been applied. We start with the previous extremities
		trace!("Calculating extremities");
		let mut forward_extremities = forward_extremities
			.into_iter()
			.stream()
			.ready_filter(|event_id| {
				// Remove any that are referenced by this incoming event's prev_events
				!incoming_pdu.prev_events().any(is_equal_to!(event_id))
			})
			.broad_filter_map(|event_id| async move {
				// Only keep those extremities were not referenced yet
				self.services
					.pdu_metadata
					.is_event_referenced(&incoming_pdu.room_id_or_hash(), &event_id)
					.await
					.eq(&false)
					.then_some(event_id)
			})
			.collect::<Vec<_>>()
			.await;
		forward_extremities.push(incoming_pdu.event_id().to_owned());
		debug!(
			"Retained {} extremities checked against {} prev_events",
			forward_extremities.len(),
			incoming_pdu.prev_events().count()
		);
		assert!(!forward_extremities.is_empty(), "resolved extremities cannot be empty");
		Ok(forward_extremities)
	}

	/// Derives a new room state by adding the incoming PDU to the state before
	/// it to create the state at, which then becomes the current room state.
	///
	/// The caller MUST ensure forward extremities are set appropriately,
	/// including this incoming pdu, either before or after calling this
	/// function. Failing to do so will result in an inconsistent current state
	/// cache, which may affect event authentication.
	async fn derive_new_state(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		state_before: HashMap<u64, OwnedEventId>,
		state_lock: &Guard<OwnedRoomId, ()>,
	) -> Result {
		let room_id = incoming_pdu.room_id_or_hash();
		// We also add state after incoming event to the fork states
		let mut state_at_incoming_event = state_before;
		let shortstatekey = self
			.services
			.short
			.get_or_create_shortstatekey(
				&incoming_pdu.kind().to_string().into(),
				incoming_pdu.state_key().unwrap(),
			)
			.await;

		let event_id = incoming_pdu.event_id();
		state_at_incoming_event.insert(shortstatekey, event_id.to_owned());

		debug!("Resolving new room state");
		let new_room_state = self
			.resolve_state(&room_id, room_version_rules, state_at_incoming_event)
			.await?;

		debug!("Forcing new room state");
		let HashSetCompressStateEvent { shortstatehash, added, removed } = self
			.services
			.state_compressor
			.save_state(&room_id, new_room_state)
			.await?;

		self.services
			.state
			.force_state(&room_id, shortstatehash, added, removed, state_lock)
			.await
	}
}
