use std::{
	collections::{BTreeMap, hash_map},
	time::Instant,
};

use conduwuit::{
	Err, Event, Result, debug::INFO_SPAN_LEVEL, defer, err, implement, info,
	utils::stream::IterStream, warn,
};
use futures::{
	FutureExt, TryFutureExt, TryStreamExt,
	future::{OptionFuture, try_join5},
};
use ruma::{CanonicalJsonValue, EventId, RoomId, ServerName, UserId, events::StateEventType};
use tracing::debug;

use crate::rooms::timeline::{RawPduId, pdu_fits};

/// When receiving an event one needs to:
/// 0. Check the server is in the room
/// 1. Skip the PDU if we already know about it
/// 1.1. Remove unsigned field
/// 2. Check signatures, otherwise drop
/// 3. Check content hash, redact if doesn't match
/// 4. Fetch any missing auth events doing all checks listed here starting at 1.
///    These are not timeline events
/// 5. Reject "due to auth events" if can't get all the auth events or some of
///    the auth events are also rejected "due to auth events"
/// 6. Reject "due to auth events" if the event doesn't pass auth based on the
///    auth events
/// 7. Persist this event as an outlier
/// 8. If not timeline event: stop
/// 9. Fetch any missing prev events doing all checks listed here starting at 1.
///    These are timeline events
/// 10. Fetch missing state and auth chain events by calling `/state_ids` at
///     backwards extremities doing all the checks in this list starting at
///     1. These are not timeline events
/// 11. Check the auth of the event passes based on the state of the event
/// 12. Ensure that the state is derived from the previous current state (i.e.
///     we calculated by doing state res where one of the inputs was a
///     previously trusted set of state, don't just trust a set of state we got
///     from a remote)
/// 13. Use state resolution to find new room state
/// 14. Check if the event passes auth based on the "current state" of the room,
///     if not soft fail it
#[implement(super::Service)]
#[tracing::instrument(
	name = "pdu",
	level = INFO_SPAN_LEVEL,
	skip_all,
	fields(%room_id, %event_id),
)]
pub async fn handle_incoming_pdu<'a>(
	&self,
	origin: &'a ServerName,
	room_id: &'a RoomId,
	event_id: &'a EventId,
	value: BTreeMap<String, CanonicalJsonValue>,
	is_timeline_event: bool,
) -> Result<Option<RawPduId>> {
	// 1. Skip the PDU if we already have it as a timeline event
	if let Ok(pdu_id) = self.services.timeline.get_pdu_id(event_id).await {
		return Ok(Some(pdu_id));
	}
	if !pdu_fits(&mut value.clone()) {
		warn!(
			"dropping incoming PDU {event_id} in room {room_id} from {origin} because it \
			 exceeds 65535 bytes or is otherwise too large."
		);
		return Err!(Request(TooLarge("PDU is too large")));
	}

	// 1.1 Check we even know about the room
	let meta_exists = self.services.metadata.exists(room_id).map(Ok);

	// 1.2 Check if the room is disabled
	let is_disabled = self.services.metadata.is_disabled(room_id).map(Ok);

	// 1.3.1 Check room ACL on origin field/server
	let origin_acl_check = self.acl_check(origin, room_id);

	// 1.3.2 Check room ACL on sender's server name
	let sender: &UserId = value
		.get("sender")
		.try_into()
		.map_err(|e| err!(Request(InvalidParam("PDU does not have a valid sender key: {e}"))))?;

	let sender_acl_check: OptionFuture<_> = sender
		.server_name()
		.ne(origin)
		.then(|| self.acl_check(sender.server_name(), room_id))
		.into();

	// Fetch create event
	let create_event =
		self.services
			.state_accessor
			.room_state_get(room_id, &StateEventType::RoomCreate, "");

	let (meta_exists, is_disabled, (), (), ref create_event) = try_join5(
		meta_exists,
		is_disabled,
		origin_acl_check,
		sender_acl_check.map(|o| o.unwrap_or(Ok(()))),
		create_event,
	)
	.await?;

	if !meta_exists {
		return Err!(Request(NotFound("Room is unknown to this server")));
	}

	if is_disabled {
		return Err!(Request(Forbidden("Federation of this room is disabled by this server.")));
	}

	if !self
		.services
		.state_cache
		.server_in_room(self.services.globals.server_name(), room_id)
		.await
	{
		info!(
			%origin,
			"Dropping inbound PDU for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	let (incoming_pdu, val) = self
		.handle_outlier_pdu(origin, create_event, event_id, room_id, value, false)
		.await?;

	// 8. if not timeline event: stop
	if !is_timeline_event {
		return Ok(None);
	}

	// Skip old events
	let first_ts_in_room = self
		.services
		.timeline
		.first_pdu_in_room(room_id)
		.await?
		.origin_server_ts();

	// 9. Fetch any missing prev events doing all checks listed here starting at 1.
	//    These are timeline events
	let (sorted_prev_events, mut eventid_info) = self
		.fetch_prev(origin, create_event, room_id, first_ts_in_room, incoming_pdu.prev_events())
		.await?;

	debug!(
		events = ?sorted_prev_events,
		"Handling previous events"
	);

	sorted_prev_events
		.iter()
		.try_stream()
		.map_ok(AsRef::as_ref)
		.try_for_each(|prev_id| {
			self.handle_prev_pdu(
				origin,
				event_id,
				room_id,
				eventid_info.remove(prev_id),
				create_event,
				first_ts_in_room,
				prev_id,
			)
			.inspect_err(move |e| {
				warn!("Prev {prev_id} failed: {e}");
				match self
					.services
					.globals
					.bad_event_ratelimiter
					.write()
					.entry(prev_id.into())
				{
					| hash_map::Entry::Vacant(e) => {
						e.insert((Instant::now(), 1));
					},
					| hash_map::Entry::Occupied(mut e) => {
						let tries = e.get().1.saturating_add(1);
						*e.get_mut() = (Instant::now(), tries);
					},
				}
			})
			.map(|_| self.services.server.check_running())
		})
		.boxed()
		.await?;

	// Done with prev events, now handling the incoming event
	let start_time = Instant::now();
	self.federation_handletime
		.write()
		.insert(room_id.into(), (event_id.to_owned(), start_time));

	defer! {{
		self.federation_handletime
			.write()
			.remove(room_id);
	}};

	self.upgrade_outlier_to_timeline_pdu(incoming_pdu, val, create_event, origin, room_id)
		.boxed()
		.await
}
