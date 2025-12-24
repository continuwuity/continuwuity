use std::collections::HashMap;

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use base64::{Engine as _, engine::general_purpose};
use conduwuit::{
	Err, Error, EventTypeExt, PduEvent, Result, RoomVersion, debug, debug_warn, err,
	matrix::{Event, StateKey, event::gen_event_id},
	trace,
	utils::{self, hash::sha256},
	warn,
};
use ruma::{
	CanonicalJsonValue, OwnedUserId, RoomId, RoomVersionId, ServerName, UserId,
	api::{client::error::ErrorKind, federation::membership::create_invite},
	events::{
		StateEventType,
		room::{
			create::RoomCreateEventContent,
			member::{MembershipState, RoomMemberEventContent},
		},
	},
};
use serde_json::value::RawValue;
use service::{Services, rooms::timeline::pdu_fits};

use crate::Ruma;

/// Ensures that the state received from the invite endpoint is sane, correct,
/// and complies with the room version's requirements.
async fn check_invite_state(
	services: &Services,
	stripped_state: &Vec<Box<RawValue>>,
	room_id: &RoomId,
	room_version_id: &RoomVersionId,
) -> Result<()> {
	let room_version = RoomVersion::new(room_version_id).map_err(|e| {
		err!(Request(UnsupportedRoomVersion("Invalid room version provided: {e}")))
	})?;
	let mut room_state: HashMap<(StateEventType, StateKey), PduEvent> = HashMap::new();

	// Build the room state from the provided state events,
	// ensuring that there's no duplicates. We need to check that m.room.create is
	// present and lines up with the other things we've been told, and then verify
	// any signatures present to ensure this isn't forged.
	for raw_event in stripped_state {
		trace!("Processing invite state event: {:?}", raw_event);
		let canonical = utils::to_canonical_object(raw_event)?;
		let event_id = gen_event_id(&canonical, room_version_id)?;
		let event = PduEvent::from_id_val(&event_id, canonical.clone())
			.map_err(|e| err!(Request(InvalidParam("Invite state event is invalid: {e}"))))?;
		if event.state_key().is_none() {
			return Err!(Request(InvalidParam("Invite state event missing event type.")));
		}
		let key = event
			.event_type()
			.with_state_key(event.state_key().unwrap());
		if room_state.contains_key(&key) {
			return Err!(Request(InvalidParam("Duplicate state event found for {key:?}")));
		}

		// verify the event
		if !pdu_fits(&canonical) {
			return Err!(Request(InvalidParam(
				"An invite state event ({event_id}) is too large"
			)));
		}
		services
			.server_keys
			.verify_event(&canonical, Some(room_version_id))
			.await
			.map_err(|e| {
				err!(Request(InvalidParam(
					"Signature failed verification on event {event_id}: {e}"
				)))
			})?;

		// Ensure all events are in the same room
		if event.room_id_or_hash() != room_id {
			return Err!(Request(InvalidParam(
				"State event room ID for {} does not match the expected room ID {}.",
				event.event_id,
				room_id,
			)));
		}
		room_state.insert(key, event);
	}

	// verify m.room.create is present, has a matching room ID, and a matching room
	// version.
	let create_event = room_state
		.get(&(StateEventType::RoomCreate, "".into()))
		.ok_or_else(|| err!(Request(MissingParam("Missing m.room.create in stripped state."))))?;
	let create_event_content: RoomCreateEventContent = create_event
		.get_content()
		.map_err(|e| err!(Request(InvalidParam("Invalid m.room.create content: {e}"))))?;
	// Room v12 removed room IDs over federation, so we'll need to see if the event
	// ID matches the room ID instead.
	if room_version.room_ids_as_hashes {
		let given_room_id = create_event.event_id().as_str().replace('$', "!");
		if given_room_id != room_id.as_str() {
			return Err!(Request(InvalidParam(
				"m.room.create event ID does not match the room ID."
			)));
		}
	} else if create_event.room_id().unwrap() != room_id {
		return Err!(Request(InvalidParam("m.room.create room ID does not match the room ID.")));
	}

	// Make sure the room version matches
	if &create_event_content.room_version != room_version_id {
		return Err!(Request(InvalidParam(
			"m.room.create room version does not match the given room version."
		)));
	}

	// Looks solid
	Ok(())
}

/// Ensures that the invite event received from the invite endpoint is sane,
/// correct, and complies with the room version's requirements.
/// Returns the invited user ID on success.
async fn check_invite_event(
	services: &Services,
	invite_event: &PduEvent,
	origin: &ServerName,
	room_id: &RoomId,
	room_version_id: &RoomVersionId,
) -> Result<OwnedUserId> {
	// Check: The event sender is not a user ID on the origin server.
	if invite_event.sender.server_name() != origin {
		return Err!(Request(InvalidParam(
			"Invite event sender's server does not match the origin server."
		)));
	}
	// Check: The `state_key` is not a user ID on the receiving server.
	let state_key: &UserId = invite_event
		.state_key()
		.ok_or_else(|| err!(Request(MissingParam("Invite event missing state_key."))))?
		.try_into()
		.map_err(|e| err!(Request(InvalidParam("Invalid state_key property: {e}"))))?;
	if !services.globals.server_is_ours(state_key.server_name()) {
		return Err!(Request(InvalidParam(
			"Invite event state_key does not belong to this homeserver."
		)));
	}

	// Check: The event's room ID matches the expected room ID.
	if let Some(evt_room_id) = invite_event.room_id() {
		if evt_room_id != room_id {
			return Err!(Request(InvalidParam(
				"Invite event room ID does not match the expected room ID."
			)));
		}
	} else {
		return Err!(Request(MissingParam("Invite event missing room ID.")));
	}

	// Check: the membership really is "invite"
	let content = invite_event.get_content::<RoomMemberEventContent>()?;
	if content.membership != MembershipState::Invite {
		return Err!(Request(InvalidParam("Invite event is not a membership invite.")));
	}

	// Check: signature is valid
	let as_obj = &mut utils::to_canonical_object(invite_event)?;
	// remove the event_id before verification
	as_obj.remove("event_id");
	services
		.server_keys
		.verify_event(as_obj, Some(room_version_id))
		.await
		.map_err(|e| {
			err!(Request(InvalidParam("Invite event signature failed verification: {e}")))
		})?;

	Ok(state_key.to_owned())
}

/// Performs only legacy checks on the invite, for use when the requesting
/// server doesn't support matrix v1.16 invites.
/// This is significantly less secure than the full checks and should only be
/// used if the user has allowed it.
async fn legacy_check_invite(
	services: &Services,
	origin: &ServerName,
	invite_event: &PduEvent,
	stripped_state: &Vec<Box<RawValue>>,
	room_id: &RoomId,
	room_version_id: &RoomVersionId,
) -> Result<OwnedUserId> {
	// Ensure the sender is from origin, the state key is a user ID that points at a
	// local user, the event type is m.room.member with membership "invite", and
	// the room ID matches.
	if invite_event.sender().server_name() != origin {
		return Err!(Request(InvalidParam(
			"Invite event sender's server does not match the origin server."
		)));
	}
	let state_key: &UserId = invite_event
		.state_key()
		.ok_or_else(|| err!(Request(MissingParam("Invite event missing state_key."))))?
		.try_into()
		.map_err(|e| err!(Request(InvalidParam("Invalid state_key property: {e}"))))?;
	if !services.globals.server_is_ours(state_key.server_name()) {
		return Err!(Request(InvalidParam(
			"Invite event state_key does not belong to this homeserver."
		)));
	}
	if let Some(evt_room_id) = invite_event.room_id() {
		if evt_room_id != room_id {
			return Err!(Request(InvalidParam(
				"Invite event room ID does not match the expected room ID."
			)));
		}
	} else {
		return Err!(Request(MissingParam("Invite event missing room ID.")));
	}
	let content = invite_event.get_content::<RoomMemberEventContent>()?;
	if content.membership != MembershipState::Invite {
		return Err!(Request(InvalidParam("Invite event is not a membership invite.")));
	}

	// We can also opportunistically check that the m.room.create event is present
	// and matches the room version, to avoid accepting invites to rooms that
	// don't match.
	let mut has_create = false;
	for raw_event in stripped_state {
		let canonical = utils::to_canonical_object(raw_event)?;
		if canonical.get("room_id").is_none() {
			// This is a stripped event, skip
			continue;
		}
		if let Some(event_type) = canonical.get("type") {
			if event_type.as_str().unwrap_or_default() == "m.room.create" {
				has_create = true;
				let event_id = gen_event_id(&canonical, room_version_id)?;
				let event = PduEvent::from_id_val(&event_id, canonical.clone()).map_err(|e| {
					err!(Request(InvalidParam("Invite state event is invalid: {e}")))
				})?;

				// We can verify that the room ID is correct now
				let version = RoomVersion::new(room_version_id)?;
				if version.room_ids_as_hashes {
					let given_room_id = event.event_id().as_str().replace('$', "!");
					if given_room_id != room_id.as_str() {
						return Err!(Request(InvalidParam(
							"m.room.create event ID does not match the room ID."
						)));
					}
				} else if event.room_id().unwrap() != room_id {
					return Err!(Request(InvalidParam(
						"m.room.create room ID does not match the room ID."
					)));
				}
				// Everything's as fine as we're getting with this event
				break;
			}
		}
	}
	if !has_create {
		warn!(
			"federated invite is missing m.room.create event in stripped state, the remote \
			 server is either outdated or trying something fishy."
		);
	}

	Ok(state_key.to_owned())
}

/// Checks the incoming event is allowed and not forged.
/// If the MSC4311 enforcement experiment is enabled, performs full checks,
/// otherwise performs legacy checks only.
async fn check_invite(
	services: &Services,
	invite_event: &PduEvent,
	stripped_state: &Vec<Box<RawValue>>,
	origin: &ServerName,
	room_id: &RoomId,
	room_version_id: &RoomVersionId,
) -> Result<OwnedUserId> {
	if services.config.experiments.enforce_msc4311 {
		debug!("Checking invite event validity");
		let user = check_invite_event(services, invite_event, origin, room_id, room_version_id)
			.await
			.inspect_err(|e| {
				debug_warn!("Invite event validity check failed: {e}");
			})?;
		debug!("Checking invite state validity");
		check_invite_state(services, stripped_state, room_id, room_version_id)
			.await
			.inspect_err(|e| {
				debug_warn!("Invite state validity check failed: {e}");
			})?;
		Ok(user)
	} else {
		debug!("Performing legacy invite checks");
		legacy_check_invite(
			services,
			origin,
			invite_event,
			stripped_state,
			room_id,
			room_version_id,
		)
		.await
		.inspect_err(|e| {
			debug_warn!("Legacy invite validity check failed: {e}");
		})
	}
}

/// # `PUT /_matrix/federation/v2/invite/{roomId}/{eventId}`
///
/// Invites a remote user to a room.
#[tracing::instrument(skip_all, fields(%client, room_id=?body.room_id), name = "invite")]
pub(crate) async fn create_invite_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<create_invite::v2::Request>,
) -> Result<create_invite::v2::Response> {
	debug!("Received invite request from {}: {:?}", body.room_id, body.origin());

	// ACL check origin
	services
		.rooms
		.event_handler
		.acl_check(body.origin(), &body.room_id)
		.await?;

	if !services.server.supported_room_version(&body.room_version) {
		debug_warn!("Unsupported room version: {}", body.room_version);
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion { room_version: body.room_version.clone() },
			"Server does not support this room version.",
		));
	}

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			warn!("Received invite to room created by a banned server: {}. Rejecting.", server);
			return Err!(Request(Forbidden("Server is banned on this homeserver.")));
		}
	}

	if services
		.moderation
		.is_remote_server_forbidden(body.origin())
	{
		warn!(
			"Received invite from banned server {} for room ID {}. Rejecting.",
			body.origin(),
			body.room_id
		);

		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	let mut signed_event = utils::to_canonical_object(&body.event)
		.map_err(|_| err!(Request(InvalidParam("Invite event is invalid."))))?;

	// We need to hash and sign the event before we can generate the event ID.
	// It is important that this signed event does not get sent back to the caller
	// until we've verified this isn't incorrect.
	trace!(event=?signed_event, "Hashing & signing invite event");
	services
		.server_keys
		.hash_and_sign_event(&mut signed_event, &body.room_version)
		.map_err(|e| err!(Request(InvalidParam("Failed to sign event: {e}"))))?;
	let event_id = gen_event_id(&signed_event.clone(), &body.room_version)?;
	if event_id != body.event_id {
		warn!("Event ID mismatch: expected {}, got {}", event_id, body.event_id);
		return Err!(Request(InvalidParam("Event ID does not match the generated event ID.")));
	}

	let pdu = PduEvent::from_id_val(&event_id, signed_event.clone())
		.map_err(|e| err!(Request(InvalidParam("Invite event is invalid: {e}"))))?;

	let recipient_user = check_invite(
		&services,
		&pdu,
		&body.invite_room_state,
		body.origin(),
		&body.room_id,
		&body.room_version,
	)
	.await?;

	// Make sure the room isn't banned and we allow invites
	if services.config.block_non_admin_invites && !services.users.is_admin(&recipient_user).await
	{
		return Err!(Request(Forbidden("This server does not allow room invites.")));
	}
	if services.rooms.metadata.is_banned(&body.room_id).await
		&& !services.users.is_admin(&recipient_user).await
	{
		return Err!(Request(Forbidden("This room is banned on this homeserver.")));
	}

	// Make sure we're not ACL'ed from their room.
	services
		.rooms
		.event_handler
		.acl_check(recipient_user.server_name(), &body.room_id)
		.await?;

	// Add event_id back
	signed_event.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.to_string()));

	// If we are active in the room, the remote server will notify us about the
	// join/invite through /send. If we are not in the room, we need to manually
	// record the invited state for client /sync through update_membership(), and
	// send the invite PDU to the relevant appservices.
	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await
	{
		let mut invite_state: Vec<CanonicalJsonValue> = body
			.invite_room_state
			.clone()
			.into_iter()
			.map(|v| utils::to_canonical_object(&v).unwrap().into())
			.collect();

		invite_state.push(pdu.to_canonical_object().into());
		let sender_user: &UserId = signed_event
			.get("sender")
			.try_into()
			.map_err(|e| err!(Request(InvalidParam("Invalid sender property: {e}"))))?;
		debug!("Marking user {} as invited to remote room {}", recipient_user, body.room_id);
		services
			.rooms
			.state_cache
			.mark_as_invited(
				&recipient_user,
				&body.room_id,
				sender_user,
				Some(invite_state),
				body.via.clone(),
			)
			.await?;

		services
			.rooms
			.state_cache
			.update_joined_count(&body.room_id)
			.await;

		for appservice in services.appservice.read().await.values() {
			if appservice.is_user_match(&recipient_user) {
				services
					.sending
					.send_appservice_request(
						appservice.registration.clone(),
						ruma::api::appservice::event::push_events::v1::Request {
							events: vec![pdu.to_format()],
							txn_id: general_purpose::URL_SAFE_NO_PAD
								.encode(sha256::hash(pdu.event_id.as_bytes()))
								.into(),
							ephemeral: Vec::new(),
							to_device: Vec::new(),
						},
					)
					.await?;
			}
		}
	}

	debug!("Invite is valid, returning signed event");
	Ok(create_invite::v2::Response {
		event: services
			.sending
			.convert_to_outgoing_federation_event(signed_event)
			.await,
	})
}
