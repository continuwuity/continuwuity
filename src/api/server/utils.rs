use conduwuit::{Err, Result, err, is_false};
use conduwuit_service::Services;
use futures::{FutureExt, future::OptionFuture, join};
use ruma::{
	CanonicalJsonObject, EventId, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, ServerName,
	UserId, events::room::member::MembershipState, room_version_rules::RoomVersionRules,
};

pub(super) struct AccessCheck<'a> {
	pub(super) services: &'a Services,
	pub(super) origin: &'a ServerName,
	pub(super) room_id: &'a RoomId,
	pub(super) event_id: Option<&'a EventId>,
}

impl AccessCheck<'_> {
	/// Asserts that the server has access to the room and event (if any).
	/// If the server is permitted, `Ok(())` is returned. Otherwise, a Forbidden
	/// error is returned.
	pub(super) async fn assert(&self) -> Result {
		let acl_check = self
			.services
			.rooms
			.event_handler
			.acl_check(self.origin, self.room_id)
			.map(|result| result.is_ok());

		let world_readable = self
			.services
			.rooms
			.state_accessor
			.is_world_readable(self.room_id);

		let server_in_room = self
			.services
			.rooms
			.state_cache
			.server_in_room(self.origin, self.room_id);

		let server_can_see: OptionFuture<_> = self
			.event_id
			.map(|event_id| {
				self.services.rooms.state_accessor.server_can_see_event(
					self.origin,
					self.room_id,
					event_id,
				)
			})
			.into();

		let (world_readable, server_in_room, server_can_see, acl_check) =
			join!(world_readable, server_in_room, server_can_see, acl_check);

		if !acl_check {
			return Err!(Request(Forbidden("Server access denied.")));
		}

		if !world_readable && !server_in_room {
			return Err!(Request(Forbidden("Server is not in room.")));
		}

		if server_can_see.is_some_and(is_false!()) {
			return Err!(Request(Forbidden("Server is not allowed to see event.")));
		}

		Ok(())
	}
}

/// Performs validation on a membership event that should be run on any event a
/// remote is trying to send via us.
///
/// ## Checks performed
///
/// 1. PDU room ID matches request path room ID
/// 2. PDU event ID matches request path event ID
/// 3. Signature check
/// 4. Event type check
/// 5. `sender` field presence (and parsing)
/// 6. `state_key` field presence (and parsing)
/// 7. PDU room format check (PDU check 1)
///
/// ## Returns
///
/// A resulting tuple of (PDU JSON, target membership state, sender, recipient).
pub(crate) async fn validate_any_membership_event(
	services: &crate::State,
	body: &serde_json::value::RawValue,
	room_version_rules: &RoomVersionRules,
	create_event_id: OwnedEventId,
	expected_room_id: OwnedRoomId,
	expected_event_id: OwnedEventId,
) -> Result<(CanonicalJsonObject, MembershipState, OwnedUserId, OwnedUserId)> {
	let (template_room_id, template_event_id, pdu) = services
		.rooms
		.event_handler
		.parse_incoming_pdu(body)
		.await
		.map_err(|e| err!(Request(BadJson("Invalid membership PDU: {e}"))))?;

	if template_room_id != expected_room_id {
		return Err!(Request(InvalidParam("Membership event does not belong to requested room")));
	}
	if template_event_id != expected_event_id {
		return Err!(Request(InvalidParam(
			"Membership event ID does not match provided event ID"
		)));
	}

	services
		.server_keys
		.verify_event(&pdu, room_version_rules)
		.await
		.map_err(|e| {
			err!(Request(InvalidParam("Signature verification failed on membership event: {e}")))
		})?;

	// Ensure this is a membership event
	if pdu
		.get("type")
		.expect("event must have a type")
		.as_str()
		.expect("type must be a string")
		!= "m.room.member"
	{
		return Err!(Request(BadJson(
			"Not allowed to send non-membership event to this endpoint"
		)));
	}
	let membership = pdu
		.get("content")
		.ok_or_else(|| err!(Request(BadJson("Event missing content property"))))?
		.as_object()
		.ok_or_else(|| err!(Request(BadJson("Event content is not an object"))))?
		.get("membership")
		.ok_or_else(|| err!(Request(BadJson("Event missing membership property"))))?
		.as_str()
		.ok_or_else(|| err!(Request(BadJson("Event is not a string"))))?
		.to_owned();

	let sender_user = pdu
		.get("sender")
		.and_then(|v| v.as_str())
		.map(UserId::parse)
		.and_then(Result::ok)
		.ok_or_else(|| err!(Request(InvalidParam("Invalid sender property"))))?;
	let recipient_user = pdu
		.get("state_key")
		.and_then(|v| v.as_str())
		.map(UserId::parse)
		.and_then(Result::ok)
		.ok_or_else(|| err!(Request(InvalidParam("Invalid state_key property"))))?;

	// Do a quick format check. The spec doesn't suggest this, but it's probably
	// a good idea nonetheless.
	service::rooms::event_handler::Service::pdu_format_check_1(
		&pdu,
		room_version_rules,
		&create_event_id,
	)
	.map_err(|e| {
		err!(Request(InvalidParam("Membership event violates the room event format: {e}")))
	})?;

	Ok((pdu, membership.into(), sender_user, recipient_user))
}
