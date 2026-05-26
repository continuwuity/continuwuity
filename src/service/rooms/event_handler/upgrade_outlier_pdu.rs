use std::{borrow::Borrow, sync::Arc, time::Instant};

use conduwuit::{
	Err, Result, debug, debug_info, err, info, is_equal_to,
	matrix::{Event, EventTypeExt, PduEvent, StateKey, state_res},
	trace,
	utils::{
		IterStream,
		stream::{BroadbandExt, ReadyExt},
	},
	warn,
};
use futures::{FutureExt, StreamExt, future::ready};
use ruma::{
	CanonicalJsonObject, RoomId, ServerName, api::error::ErrorKind, events::StateEventType,
};
use tokio::join;

use super::get_room_version_rules;
use crate::rooms::{
	state_compressor::{CompressedState, HashSetCompressStateEvent},
	timeline::RawPduId,
};

impl super::Service {
	pub(super) async fn upgrade_outlier_to_timeline_pdu<Pdu>(
		&self,
		incoming_pdu: PduEvent,
		mut val: CanonicalJsonObject,
		create_event: &Pdu,
		origin: &ServerName,
		room_id: &RoomId,
	) -> Result<Option<RawPduId>>
	where
		Pdu: Event + Send + Sync,
	{
		// Skip the PDU if we already have it as a timeline event
		if let Ok(pduid) = self
			.services
			.timeline
			.get_pdu_id(incoming_pdu.event_id())
			.await
		{
			trace!(event_id=%incoming_pdu.event_id(), "Skipping upgrade of already upgraded PDU");
			return Ok(Some(pduid));
		}

		let (rejected, soft_failed) = join!(
			self.services
				.pdu_metadata
				.is_event_rejected(incoming_pdu.event_id()),
			self.services
				.pdu_metadata
				.is_event_soft_failed(incoming_pdu.event_id())
		);
		if rejected {
			return Err!(Request(InvalidParam("Event has been rejected")));
		} else if soft_failed {
			return Err!(Request(InvalidParam("Event has been soft-failed")));
		}

		debug!(
			event_id = %incoming_pdu.event_id,
			"Upgrading PDU from outlier to timeline"
		);
		let timer = Instant::now();
		let min_depth = self.services.metadata.get_mindepth(room_id).await;
		let room_version_rules = get_room_version_rules(create_event)?;

		// 10. Fetch missing state and auth chain events by calling /state_ids at
		//     backwards extremities doing all the checks in this list starting at 1.
		//     These are not timeline events.

		debug!(
			event_id = %incoming_pdu.event_id,
			"Resolving state at event"
		);
		let mut state_at_incoming_event = if incoming_pdu.prev_events().count() == 1 {
			self.state_at_incoming_degree_one(&incoming_pdu).await?
		} else {
			self.state_at_incoming_resolved(&incoming_pdu, room_id, &room_version_rules)
				.await?
		};

		if state_at_incoming_event.is_none() {
			trace!("Could not calculate incoming state, asking remote {origin} for it");
			state_at_incoming_event = self
				.fetch_state(origin, create_event, room_id, incoming_pdu.event_id())
				.await?;
		}

		let state_at_incoming_event =
			state_at_incoming_event.expect("we always set this to some above");

		debug!(
			event_id = %incoming_pdu.event_id,
			"Performing auth check to upgrade"
		);
		// 11. Check the auth of the event passes based on the state of the event
		let state_fetch_state = &state_at_incoming_event;
		let state_fetch = |k: StateEventType, s: StateKey| async move {
			let shortstatekey = self.services.short.get_shortstatekey(&k, &s).await.ok()?;

			let event_id = state_fetch_state.get(&shortstatekey)?;
			self.services.timeline.get_pdu(event_id).await.ok()
		};

		debug!(
			event_id = %incoming_pdu.event_id,
			"Running initial auth check"
		);
		// PDU check: 5
		let auth_check = state_res::event_auth::auth_check(
			&room_version_rules,
			&incoming_pdu,
			None, // TODO: third party invite
			|ty, sk| state_fetch(ty.clone(), sk.into()),
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;

		if !auth_check {
			self.services
				.pdu_metadata
				.mark_event_rejected(incoming_pdu.event_id());
			return Err!(Request(Forbidden(
				"Event authorisation fails based on the state before the event"
			)));
		}

		debug!(
			event_id = %incoming_pdu.event_id,
			"Gathering auth events"
		);
		let auth_events = self
			.services
			.state
			.get_auth_events(
				room_id,
				incoming_pdu.kind(),
				incoming_pdu.sender(),
				incoming_pdu.state_key(),
				incoming_pdu.content(),
				&room_version_rules,
			)
			.await?;

		let state_fetch = |k: &StateEventType, s: &str| {
			let key = k.with_state_key(s);
			ready(auth_events.get(&key).map(ToOwned::to_owned))
		};

		debug!(
			event_id = %incoming_pdu.event_id,
			"Running auth check with claimed state auth"
		);
		// PDU check: 6
		let auth_check = state_res::event_auth::auth_check(
			&room_version_rules,
			&incoming_pdu,
			None, // third-party invite
			state_fetch,
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;
		if !auth_check {
			warn!(
				event_id = %incoming_pdu.event_id,
				"Event authorization fails based on the current state of the room"
			);
		}

		// Soft fail check before doing state res
		debug!(
			event_id = %incoming_pdu.event_id,
			"Performing soft-fail check"
		);
		let mut soft_fail = match (auth_check, incoming_pdu.redacts_id(&room_version_rules)) {
			| (false, _) => true,
			| (true, None) => false,
			| (true, Some(redact_id)) => {
				if !self
					.services
					.state_accessor
					.user_can_redact(&redact_id, incoming_pdu.sender(), room_id, true)
					.await?
				{
					warn!(redacts = %redact_id, "User is not allowed to redact event");
					true
				} else {
					false
				}
			},
		};

		// 13. Use state resolution to find new room state
		// We start looking at current room state now, so lets lock the room
		trace!(
			room_id = %room_id,
			"Locking the room"
		);
		let state_lock = self.services.state.mutex.lock(room_id).await;

		let state_ids_compressed: Arc<CompressedState> = self
			.services
			.state_compressor
			.compress_state_events(
				state_at_incoming_event
					.iter()
					.map(|(ssk, eid)| (ssk, eid.borrow())),
			)
			.collect()
			.map(Arc::new)
			.await;

		if incoming_pdu.state_key().is_some() {
			debug!("Event is a state-event. Deriving new room state");

			// We also add state after incoming event to the fork states
			let mut state_after = state_at_incoming_event.clone();
			if let Some(state_key) = incoming_pdu.state_key() {
				let shortstatekey = self
					.services
					.short
					.get_or_create_shortstatekey(
						&incoming_pdu.kind().to_string().into(),
						state_key,
					)
					.await;

				let event_id = incoming_pdu.event_id();
				state_after.insert(shortstatekey, event_id.to_owned());
			}

			let new_room_state = self
				.resolve_state(room_id, &room_version_rules, state_after)
				.await?;

			// Set the new room state to the resolved state
			debug!("Forcing new room state");
			let HashSetCompressStateEvent { shortstatehash, added, removed } = self
				.services
				.state_compressor
				.save_state(room_id, new_room_state)
				.await?;

			self.services
				.state
				.force_state(room_id, shortstatehash, added, removed, &state_lock)
				.await?;
		}

		if !soft_fail {
			// Don't call the below checks on events that have already soft-failed, there's
			// no reason to re-calculate that.
			// 14-pre. ask the policy server to sign the event, if possible
			debug!(event_id = %incoming_pdu.event_id, "Checking policy server for event");
			let tmp_evt_id = val.remove("event_id");
			if let Err(e) = self
				.policy_server_allows_event(
					&incoming_pdu,
					&mut val,
					room_id,
					&room_version_rules,
					true,
				)
				.await
			{
				if matches!(e.kind(), ErrorKind::Forbidden) {
					info!(
						event_id = %incoming_pdu.event_id,
						error = %e,
						"Event has been marked as spam by policy server: {}",
						e.message(),
					);
					soft_fail = true;
				} else {
					return Err(e);
				}
			} else {
				debug!(
					event_id = %incoming_pdu.event_id,
					"Event has passed policy server check."
				);
			}
			if let Some(id) = tmp_evt_id {
				val.insert("event_id".to_owned(), id);
			}

			// Additionally, if this is a redaction for a soft-failed event, we soft-fail it
			// also.

			// TODO: this is supposed to hide redactions from policy servers, however, for
			// full efficacy it also needs to hide redactions for unknown events. This
			// needs to be investigated at a later time.
			if let Some(redact_id) = incoming_pdu.redacts_id(&room_version_rules) {
				debug!(
					redact_id = %redact_id,
					"Checking if redaction is for a soft-failed event"
				);
				if self
					.services
					.pdu_metadata
					.is_event_soft_failed(&redact_id)
					.await
				{
					info!(
						redact_id = %redact_id,
						"Redaction is for a soft-failed event"
					);
					soft_fail = true;
				}
			}
		}

		trace!("Appending pdu to timeline");
		let mut extremities: Vec<_> = self
			.services
			.state
			.get_forward_extremities(room_id)
			.collect()
			.await;
		if !soft_fail {
			// Per https://spec.matrix.org/unstable/server-server-api/#soft-failure, soft-failed events
			// are not added as forward extremities.

			// Now we calculate the set of extremities this room has after the incoming
			// event has been applied. We start with the previous extremities (aka leaves)
			trace!("Calculating extremities");
			extremities = extremities
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
						.is_event_referenced(room_id, &event_id)
						.await
						.eq(&false)
						.then_some(event_id)
				})
				.collect::<Vec<_>>()
				.await;
			extremities.push(incoming_pdu.event_id().to_owned());
			debug!(
				"Retained {} extremities checked against {} prev_events",
				extremities.len(),
				incoming_pdu.prev_events().count()
			);
			assert!(!extremities.is_empty(), "extremities must not empty");
		}

		let pdu_id = self
			.services
			.timeline
			.append_incoming_pdu(
				&incoming_pdu,
				val,
				extremities.iter().map(Borrow::borrow),
				state_ids_compressed,
				soft_fail,
				&state_lock,
				room_id,
			)
			.await?;
		if soft_fail {
			self.services
				.pdu_metadata
				.mark_event_soft_failed(incoming_pdu.event_id());

			info!(
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
		}

		Ok(pdu_id)
	}
}
