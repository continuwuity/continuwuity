use std::{
	collections::{BTreeMap, hash_map},
	time::Instant,
};

use conduwuit::{
	Err, Event, PduEvent, Result, debug::INFO_SPAN_LEVEL, debug_error, debug_info, defer, err,
	implement, info, trace, utils::stream::IterStream, warn,
};
use futures::{
	FutureExt, TryFutureExt, TryStreamExt,
	future::{OptionFuture, try_join4},
};
use ruma::{
	CanonicalJsonValue, EventId, OwnedUserId, RoomId, ServerName, UserId,
	events::{
		StateEventType, TimelineEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
};
use tracing::debug;

use crate::rooms::timeline::{RawPduId, pdu_fits};

async fn should_rescind_invite(
	services: &crate::rooms::event_handler::Services,
	content: &mut BTreeMap<String, CanonicalJsonValue>,
	sender: &UserId,
	room_id: &RoomId,
) -> Result<Option<PduEvent>> {
	// We insert a bogus event ID since we can't actually calculate the right one
	content.insert("event_id".to_owned(), CanonicalJsonValue::String("$rescind".to_owned()));
	let pdu_event = serde_json::from_value::<PduEvent>(
		serde_json::to_value(&content).expect("CanonicalJsonObj is a valid JsonValue"),
	)
	.map_err(|e| err!("invalid PDU: {e}"))?;

	if pdu_event.room_id().is_none_or(|r| r != room_id)
		|| pdu_event.sender() != sender
		|| pdu_event.event_type() != &TimelineEventType::RoomMember
		|| pdu_event.state_key().is_none_or(|v| v == sender.as_str())
	{
		return Ok(None);
	}

	let target_user_id = UserId::parse(pdu_event.state_key().unwrap())?;
	if pdu_event
		.get_content::<RoomMemberEventContent>()?
		.membership
		!= MembershipState::Leave
	{
		return Ok(None); // Not a leave event
	}

	// Does the target user have a pending invite?
	let Ok(pending_invite_state) = services
		.state_cache
		.invite_state(target_user_id, room_id)
		.await
	else {
		return Ok(None); // No pending invite, so nothing to rescind
	};
	for event in pending_invite_state {
		if event
			.get_field::<String>("type")?
			.is_none_or(|t| t != "m.room.member")
			|| event
				.get_field::<OwnedUserId>("state_key")?
				.is_none_or(|s| s != *target_user_id)
			|| event
				.get_field::<OwnedUserId>("sender")?
				.is_none_or(|s| s != *sender)
			|| event
				.get_field::<RoomMemberEventContent>("content")?
				.is_none_or(|c| c.membership != MembershipState::Invite)
		{
			continue;
		}
		return Ok(Some(pdu_event)); // Found a pending invite, so this is a rescind
	}

	Ok(None)
}

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
	trace!("processing incoming pdu from {origin} for room {room_id} with event id {event_id}");

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

	let (meta_exists, is_disabled, (), ()) = try_join4(
		meta_exists,
		is_disabled,
		origin_acl_check,
		sender_acl_check.map(|o| o.unwrap_or(Ok(()))),
	)
	.await
	.inspect_err(|e| debug_error!("failed to handle incoming PDU: {e}"))?;

	if is_disabled {
		return Err!(Request(Forbidden("Federation of this room is disabled by this server.")));
	}

	if !self
		.services
		.state_cache
		.server_in_room(self.services.globals.server_name(), room_id)
		.await
	{
		// Is this a federated invite rescind?
		// copied from https://github.com/element-hq/synapse/blob/7e4588a/synapse/handlers/federation_event.py#L255-L300
		if value.get("type").and_then(|t| t.as_str()) == Some("m.room.member") {
			if let Some(pdu) =
				should_rescind_invite(&self.services, &mut value.clone(), sender, room_id).await?
			{
				debug_info!(
					"Invite to {room_id} appears to have been rescinded by {sender}, marking as \
					 left"
				);
				self.services
					.state_cache
					.mark_as_left(sender, room_id, Some(pdu))
					.await;
				return Ok(None);
			}
		}
		info!(
			%origin,
			"Dropping inbound PDU for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	if !meta_exists {
		return Err!(Request(NotFound("Room is unknown to this server")));
	}

	// Fetch create event
	let create_event = &(self
		.services
		.state_accessor
		.room_state_get(room_id, &StateEventType::RoomCreate, "")
		.await?);

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
