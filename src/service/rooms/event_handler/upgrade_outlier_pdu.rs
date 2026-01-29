use std::{borrow::Borrow, collections::BTreeMap, iter::once, sync::Arc, time::Instant};

use conduwuit::{
	Err, Result, debug, debug_info, err, implement, info, is_equal_to,
	matrix::{Event, EventTypeExt, PduEvent, StateKey, state_res},
	trace,
	utils::stream::{BroadbandExt, ReadyExt},
	warn,
};
use futures::{FutureExt, StreamExt, future::ready};
use ruma::{CanonicalJsonValue, RoomId, ServerName, events::StateEventType};

use super::{get_room_version_id, to_room_version};
use crate::rooms::{
	state_compressor::{CompressedState, HashSetCompressStateEvent},
	timeline::RawPduId,
};

#[implement(super::Service)]
pub(super) async fn upgrade_outlier_to_timeline_pdu<Pdu>(
	&self,
	incoming_pdu: PduEvent,
	val: BTreeMap<String, CanonicalJsonValue>,
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
		return Ok(Some(pduid));
	}

	if self
		.services
		.pdu_metadata
		.is_event_soft_failed(incoming_pdu.event_id())
		.await
	{
		return Err!(Request(InvalidParam("Event has been soft failed")));
	}

	debug!(
		event_id = %incoming_pdu.event_id,
		"Upgrading PDU from outlier to timeline"
	);
	let timer = Instant::now();
	let room_version_id = get_room_version_id(create_event)?;

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
		self.state_at_incoming_resolved(&incoming_pdu, room_id, &room_version_id)
			.await?
	};

	if state_at_incoming_event.is_none() {
		state_at_incoming_event = self
			.fetch_state(origin, create_event, room_id, incoming_pdu.event_id())
			.await?;
	}

	let state_at_incoming_event =
		state_at_incoming_event.expect("we always set this to some above");

	let room_version = to_room_version(&room_version_id);

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
	let auth_check = state_res::event_auth::auth_check(
		&room_version,
		&incoming_pdu,
		|ty, sk| state_fetch(ty.clone(), sk.into()),
		create_event.as_pdu(),
	)
	.await
	.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;

	if !auth_check {
		return Err!(Request(Forbidden("Event has failed auth check with state at the event.")));
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
			&room_version,
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
	let auth_check = state_res::event_auth::auth_check(
		&room_version,
		&incoming_pdu,
		state_fetch,
		create_event.as_pdu(),
	)
	.await
	.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;

	// Soft fail check before doing state res
	debug!(
		event_id = %incoming_pdu.event_id,
		"Performing soft-fail check"
	);
	let mut soft_fail = match (auth_check, incoming_pdu.redacts_id(&room_version_id)) {
		| (false, _) => true,
		| (true, None) => false,
		| (true, Some(redact_id)) =>
			!self
				.services
				.state_accessor
				.user_can_redact(&redact_id, incoming_pdu.sender(), room_id, true)
				.await?,
	};

	// 13. Use state resolution to find new room state

	// We start looking at current room state now, so lets lock the room
	trace!(
		room_id = %room_id,
		"Locking the room"
	);
	let state_lock = self.services.state.mutex.lock(room_id).await;

	// Now we calculate the set of extremities this room has after the incoming
	// event has been applied. We start with the previous extremities (aka leaves)
	trace!("Calculating extremities");
	let mut extremities: Vec<_> = self
		.services
		.state
		.get_forward_extremities(room_id)
		.map(ToOwned::to_owned)
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
		.collect()
		.await;
	extremities.push(incoming_pdu.event_id().to_owned());

	debug!(
		"Retained {} extremities checked against {} prev_events",
		extremities.len(),
		incoming_pdu.prev_events().count()
	);

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
				.get_or_create_shortstatekey(&incoming_pdu.kind().to_string().into(), state_key)
				.await;

			let event_id = incoming_pdu.event_id();
			state_after.insert(shortstatekey, event_id.to_owned());
		}

		let new_room_state = self
			.resolve_state(room_id, &room_version_id, state_after)
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
		// 14-pre. If the event is not a state event, ask the policy server about it
		if incoming_pdu.state_key.is_none() {
			debug!(event_id = %incoming_pdu.event_id, "Checking policy server for event");
			match self
				.ask_policy_server(
					&incoming_pdu,
					&mut incoming_pdu.to_canonical_object(),
					room_id,
					true,
				)
				.await
			{
				| Ok(false) => {
					warn!(
						event_id = %incoming_pdu.event_id,
						"Event has been marked as spam by policy server"
					);
					soft_fail = true;
				},
				| _ => {
					debug!(
						event_id = %incoming_pdu.event_id,
						"Event has passed policy server check or the policy server was unavailable."
					);
				},
			}
		}

		// Additionally, if this is a redaction for a soft-failed event, we soft-fail it
		// also.

		// TODO: this is supposed to hide redactions from policy servers, however, for
		// full efficacy it also needs to hide redactions for unknown events. This
		// needs to be investigated at a later time.
		if let Some(redact_id) = incoming_pdu.redacts_id(&room_version_id) {
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
				warn!(
					redact_id = %redact_id,
					"Redaction is for a soft-failed event, soft failing the redaction"
				);
				soft_fail = true;
			}
		}
	}

	// 14. Check if the event passes auth based on the "current state" of the room,
	//     if not soft fail it
	if soft_fail {
		info!(
			event_id = %incoming_pdu.event_id,
			"Soft failing event"
		);
		// assert!(extremities.is_empty(), "soft_fail extremities empty");
		let extremities = extremities.iter().map(Borrow::borrow);
		debug_assert!(extremities.clone().count() > 0, "extremities not empty");

		self.services
			.timeline
			.append_incoming_pdu(
				&incoming_pdu,
				val,
				extremities,
				state_ids_compressed,
				soft_fail,
				&state_lock,
				room_id,
			)
			.await?;

		// Soft fail, we keep the event as an outlier but don't add it to the timeline
		self.services
			.pdu_metadata
			.mark_event_soft_failed(incoming_pdu.event_id());

		warn!(
			event_id = %incoming_pdu.event_id,
			"Event was soft failed"
		);
		return Err!(Request(InvalidParam("Event has been soft failed")));
	}

	// Now that the event has passed all auth it is added into the timeline.
	// We use the `state_at_event` instead of `state_after` so we accurately
	// represent the state for this event.
	trace!("Appending pdu to timeline");
	let extremities = extremities
		.iter()
		.map(Borrow::borrow)
		.chain(once(incoming_pdu.event_id()));
	debug_assert!(extremities.clone().count() > 0, "extremities not empty");

	let pdu_id = self
		.services
		.timeline
		.append_incoming_pdu(
			&incoming_pdu,
			val,
			extremities,
			state_ids_compressed,
			soft_fail,
			&state_lock,
			room_id,
		)
		.await?;

	// Event has passed all auth/stateres checks
	drop(state_lock);
	debug_info!(
		elapsed = ?timer.elapsed(),
		"Accepted",
	);

	Ok(pdu_id)
}
