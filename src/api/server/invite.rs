use std::collections::{HashMap, hash_map::Entry};

use axum::extract::State;
use axum_client_ip::ClientIp;
use base64::{Engine as _, engine::general_purpose};
use conduwuit::{
	Err, Error, EventTypeExt, PduEvent, Result, err, error,
	matrix::{Event, StateKey, event::gen_event_id},
	utils::hash::sha256,
	warn,
};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, OwnedRoomId, OwnedUserId, ServerName,
	UserId,
	api::{
		error::{ErrorKind, IncompatibleRoomVersionErrorData},
		federation::membership::{RawStrippedState, create_invite},
	},
	events::{StateEventType, room::member::MembershipState},
	room_version_rules::RoomVersionRules,
	serde::JsonObject,
};

use crate::Ruma;

/// # `PUT /_matrix/federation/v2/invite/{roomId}/{eventId}`
///
/// Invites a remote user to a room.
#[tracing::instrument(skip_all, fields(%client), name = "invite", level = "info")]
pub(crate) async fn create_invite_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<create_invite::v2::Request>,
) -> Result<create_invite::v2::Response> {
	// ACL check origin
	services
		.rooms
		.event_handler
		.acl_check(&body.identity, &body.room_id)
		.await?;

	if !services.server.supported_room_version(&body.room_version) {
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData::new(
				body.room_version.clone(),
			)),
			"Server does not support this room version.",
		));
	}

	let room_version_rules = body.room_version.rules().unwrap();

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			return Err!(Request(Forbidden("Server is banned on this homeserver.")));
		}
	}

	if services
		.moderation
		.is_remote_server_forbidden(&body.identity)
	{
		warn!(
			"Received federated/remote invite from banned server {} for room ID {}. Rejecting.",
			body.identity, body.room_id
		);

		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	// First, validate the invite room state, so we can compare with the create
	// event.
	let (create_event_id, _) = validate_invite_state(
		&services,
		&body.invite_room_state,
		&room_version_rules,
		body.room_id.clone(),
	)
	.await?;

	// And then we can validate the member event itself
	let (mut signed_event, sender_user, recipient_user) = validate_membership_event(
		&services,
		&body.event,
		&room_version_rules,
		&body.identity,
		create_event_id,
		body.room_id.clone(),
		body.event_id.clone(),
	)
	.await?;

	if services.rooms.metadata.is_banned(&body.room_id).await
		&& !services.users.is_admin(&recipient_user).await
	{
		return Err!(Request(Forbidden("This room is banned on this homeserver.")));
	}

	if services.config.block_non_admin_invites && !services.users.is_admin(&recipient_user).await
	{
		return Err!(Request(Forbidden("This server does not allow room invites.")));
	}

	if let Err(e) = services
		.antispam
		.user_may_invite(sender_user.clone(), recipient_user.clone(), body.room_id.clone())
		.await
	{
		warn!("Antispam rejected invite: {e:?}");
		return Err!(Request(Forbidden("Invite rejected by antispam service.")));
	}

	// Make sure we're not ACL'ed from their room.
	services
		.rooms
		.event_handler
		.acl_check(recipient_user.server_name(), &body.room_id)
		.await?;

	services
		.server_keys
		.hash_and_sign_event(&mut signed_event, &room_version_rules)
		.map_err(|e| err!(Request(InvalidParam("Failed to sign event: {e}"))))?;

	// Generate event id
	let event_id = gen_event_id(&signed_event, &room_version_rules)?;

	// Add event_id back
	signed_event.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.to_string()));

	let mut invite_state = body.invite_room_state.clone();

	let mut event: JsonObject = serde_json::from_str(body.event.get())
		.map_err(|e| err!(Request(BadJson("Invalid invite event PDU: {e}"))))?;

	event.insert("event_id".to_owned(), "$placeholder".into());

	let pdu: PduEvent = serde_json::from_value(event.into())
		.map_err(|e| err!(Request(BadJson("Invalid invite event PDU: {e}"))))?;

	invite_state.push(RawStrippedState::Pdu(
		serde_json::value::to_raw_value(&pdu).expect("PDU was just created, it must be valid"),
	));

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
		services
			.rooms
			.state_cache
			.mark_as_invited(
				&recipient_user,
				&body.room_id,
				&sender_user,
				invite_state,
				body.via.clone(),
			)
			.await?;

		services
			.rooms
			.state_cache
			.update_joined_count(&body.room_id)
			.await;

		services.sync.wake(&recipient_user).await;

		for appservice in services.appservice.read().await.values() {
			if appservice.is_user_match(&recipient_user) {
				let transaction_id = general_purpose::URL_SAFE_NO_PAD
					.encode(sha256::hash(pdu.event_id.as_bytes()))
					.into();

				let request = ruma::api::appservice::event::push_events::v1::Request::new(
					transaction_id,
					vec![pdu.to_format()],
				);

				services
					.sending
					.send_appservice_request(appservice.registration.clone(), request)
					.await
					.map_err(|e| {
						error!(
							"failed to notify appservice {} about incoming invite: {e}",
							appservice.registration.id
						);
						err!(BadServerResponse(
							"Failed to notify appservice about incoming invite."
						))
					})?;
			}
		}
	}

	Ok(create_invite::v2::Response::new(
		services
			.sending
			.convert_to_outgoing_federation_event(signed_event)
			.await,
	))
}

/// Validates the *membership event* in the invite request, per the steps listed
/// under the invite endpoint's [spec].
///
/// Returns the validated JSON body, sender user ID, and recipient user ID.
///
/// Since this function performs a PDU format check, the create event must be
/// known ahead of time. This implies validating the invite state before the
/// invite event itself.
///
/// [spec]: https://spec.matrix.org/v1.19/server-server-api/#put_matrixfederationv2inviteroomideventid
async fn validate_membership_event(
	services: &crate::State,
	body: &serde_json::value::RawValue,
	room_version_rules: &RoomVersionRules,
	origin: &ServerName,
	create_event_id: OwnedEventId,
	room_id: OwnedRoomId,
	event_id: OwnedEventId,
) -> Result<(CanonicalJsonObject, OwnedUserId, OwnedUserId)> {
	let (template_room_id, template_event_id, pdu) = services
		.rooms
		.event_handler
		.parse_incoming_pdu(body)
		.await
		.map_err(|e| err!(Request(BadJson("Invalid invite event PDU: {e}"))))?;

	if template_room_id != room_id {
		return Err!(Request(InvalidParam("Membership event does not belong to requested room")));
	}
	if template_event_id != event_id {
		return Err!(Request(InvalidParam(
			"Membership event ID does not match provided event ID"
		)));
	}

	services
		.server_keys
		.verify_event(&pdu, room_version_rules)
		.await
		.map_err(|e| {
			err!(Request(InvalidParam("Signature verification failed on invite event: {e}")))
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
			"Not allowed to send non-membership event to invite endpoint"
		)));
	}

	// Ensure it is an invite event
	// My Huge Chain (avoids deser)
	let membership = pdu
		.get("content")
		.ok_or_else(|| err!(Request(BadJson("Event missing content property"))))?
		.as_object()
		.ok_or_else(|| err!(Request(BadJson("Event content is not an object"))))?
		.get("membership")
		.ok_or_else(|| err!(Request(BadJson("Event missing membership property"))))?
		.as_str()
		.ok_or_else(|| err!(Request(BadJson("Event is not a string"))))?;
	if MembershipState::Invite != membership.into() {
		return Err!(Request(BadJson(
			"Not allowed to send non-invite membership event to invite endpoint"
		)));
	}

	// Ensure the sender belongs to the remote that is sending the invite
	let sender_user = pdu
		.get("sender")
		.and_then(|v| v.as_str())
		.map(UserId::parse)
		.and_then(Result::ok)
		.ok_or_else(|| err!(Request(InvalidParam("Invalid sender property"))))?;

	if sender_user.server_name() != origin {
		return Err!(Request(Forbidden("Sender belongs to a different server")));
	}

	// Ensure the target user belongs to this server
	let recipient_user = pdu
		.get("state_key")
		.and_then(|v| v.as_str())
		.map(UserId::parse)
		.and_then(Result::ok)
		.ok_or_else(|| err!(Request(InvalidParam("Invalid state_key property"))))?;

	if !services
		.globals
		.server_is_ours(recipient_user.server_name())
	{
		return Err!(Request(InvalidParam("Recipient does not belong to this homeserver")));
	}

	// Do a quick format check. The spec doesn't suggest this, but it's probably
	// a good idea nonetheless.
	service::rooms::event_handler::Service::pdu_format_check_1(
		&pdu,
		room_version_rules,
		&create_event_id,
	)
	.map_err(|e| {
		err!(Request(InvalidParam(
			"Invite membership event violates the room event format: {e}"
		)))
	})?;

	Ok((pdu, sender_user, recipient_user))
}

/// Validates the *invite state* of an invite request, per the steps listed
/// under the endpoint's [spec].
///
/// Returns the create event's event ID, and the partial state map.
///
/// [spec]: https://spec.matrix.org/v1.19/server-server-api/#put_matrixfederationv2inviteroomideventid
async fn validate_invite_state(
	services: &crate::State,
	invite_state: &[RawStrippedState],
	room_version_rules: &RoomVersionRules,
	room_id: OwnedRoomId,
) -> Result<(OwnedEventId, HashMap<(StateEventType, StateKey), CanonicalJsonObject>)> {
	let mut invite_state_map: HashMap<(StateEventType, StateKey), _> =
		HashMap::with_capacity(invite_state.len());
	let mut create_event_id: Option<OwnedEventId> = None;
	for (idx, invite_state_event) in invite_state.iter().cloned().enumerate() {
		// Stripped state hasn't been sent over federation since v1.16.
		let RawStrippedState::Pdu(raw_pdu) = invite_state_event else {
			return Err!(Request(InvalidParam(
				"PDU in invite state (index {idx}) violates the room event format"
			)));
		};
		let (state_event_room_id, state_event_id, state_event_json) = services
			.rooms
			.event_handler
			.parse_incoming_pdu(&raw_pdu)
			.await
			.map_err(|e| err!(Request(InvalidParam("Invalid PDU in invite state: {e}"))))?;

		if state_event_room_id != room_id {
			return Err!(Request(InvalidParam(
				"PDU in invite state ({state_event_id}) belongs to the wrong room"
			)));
		}

		services
			.server_keys
			.verify_event(&state_event_json, room_version_rules)
			.await
			.map_err(|e| {
				err!(Request(InvalidParam("Signature verification failed on invite event: {e}")))
			})?;

		let Some(state_key) = state_event_json.get("state_key").and_then(|k| k.as_str()) else {
			return Err!(Request(InvalidParam(
				"PDU in invite state ({state_event_id}) is not a state event"
			)));
		};
		let Some(event_type) = state_event_json.get("event_type").and_then(|k| k.as_str()) else {
			return Err!(Request(InvalidParam(
				"PDU in invite state ({state_event_id}) is not an event?"
			)));
		};

		let key = StateEventType::from(event_type).with_state_key(state_key);
		match invite_state_map.entry(key) {
			| Entry::Occupied(entry) =>
				return Err!(Request(InvalidParam(
					"Duplicate state events in invite state for state key: {:?}",
					entry.key(),
				))),
			| Entry::Vacant(entry) => {
				if entry.key().0 == StateEventType::RoomCreate {
					create_event_id = Some(state_event_id);
				}
				entry.insert(state_event_json);
			},
		}
	}
	let Some(create_event_id) = create_event_id else {
		return Err!(Request(InvalidParam(
			"Invite state does not contain the m.room.create event"
		)));
	};
	invite_state_map.iter().try_for_each(|(key, event_json)| {
		service::rooms::event_handler::Service::pdu_format_check_1(
			event_json,
			room_version_rules,
			&create_event_id,
		)
		.map_err(|e| {
			err!(Request(InvalidParam(
				"PDU in invite state for {key:?} violates the room event format: {e}"
			)))
		})
	})?;

	Ok((create_event_id, invite_state_map))
}
