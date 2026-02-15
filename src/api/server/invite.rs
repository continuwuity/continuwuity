use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use base64::{Engine as _, engine::general_purpose};
use conduwuit::{
	Err, Error, PduEvent, Result, err, error,
	matrix::{Event, event::gen_event_id},
	utils::{self, hash::sha256},
	warn,
};
use ruma::{
	CanonicalJsonValue, OwnedUserId, UserId,
	api::{client::error::ErrorKind, federation::membership::create_invite},
	events::room::member::{MembershipState, RoomMemberEventContent},
	serde::JsonObject,
};

use crate::Ruma;

/// # `PUT /_matrix/federation/v2/invite/{roomId}/{eventId}`
///
/// Invites a remote user to a room.
#[tracing::instrument(skip_all, fields(%client), name = "invite", level = "info")]
pub(crate) async fn create_invite_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<create_invite::v2::Request>,
) -> Result<create_invite::v2::Response> {
	// ACL check origin
	services
		.rooms
		.event_handler
		.acl_check(body.origin(), &body.room_id)
		.await?;

	if !services.server.supported_room_version(&body.room_version) {
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion { room_version: body.room_version.clone() },
			"Server does not support this room version.",
		));
	}

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			return Err!(Request(Forbidden("Server is banned on this homeserver.")));
		}
	}

	if services
		.moderation
		.is_remote_server_forbidden(body.origin())
	{
		warn!(
			"Received federated/remote invite from banned server {} for room ID {}. Rejecting.",
			body.origin(),
			body.room_id
		);

		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	let mut signed_event = utils::to_canonical_object(&body.event)
		.map_err(|_| err!(Request(InvalidParam("Invite event is invalid."))))?;

	// Ensure this is a membership event
	if signed_event
		.get("type")
		.expect("event must have a type")
		.as_str()
		.expect("type must be a string")
		!= "m.room.member"
	{
		return Err!(Request(BadJson(
			"Not allowed to send non-membership event to invite endpoint."
		)));
	}

	let content: RoomMemberEventContent = serde_json::from_value(
		signed_event
			.get("content")
			.ok_or_else(|| err!(Request(BadJson("Event missing content property"))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson(warn!("Event content is empty or invalid: {e}")))))?;

	// Ensure this is an invite membership event
	if content.membership != MembershipState::Invite {
		return Err!(Request(BadJson(
			"Not allowed to send a non-invite membership event to invite endpoint."
		)));
	}

	// Ensure the sending user isn't a lying bozo
	let sender_server = signed_event
		.get("sender")
		.try_into()
		.map(UserId::server_name)
		.map_err(|e| err!(Request(InvalidParam("Invalid sender property: {e}"))))?;
	if sender_server != body.origin() {
		return Err!(Request(Forbidden("Sender's server does not match the origin server.",)));
	}

	// Ensure the target user belongs to this server
	let recipient_user: OwnedUserId = signed_event
		.get("state_key")
		.try_into()
		.map(UserId::to_owned)
		.map_err(|e| err!(Request(InvalidParam("Invalid state_key property: {e}"))))?;

	if !services
		.globals
		.server_is_ours(recipient_user.server_name())
	{
		return Err!(Request(InvalidParam("User does not belong to this homeserver.")));
	}

	// Make sure we're not ACL'ed from their room.
	services
		.rooms
		.event_handler
		.acl_check(recipient_user.server_name(), &body.room_id)
		.await?;

	services
		.server_keys
		.hash_and_sign_event(&mut signed_event, &body.room_version)
		.map_err(|e| err!(Request(InvalidParam("Failed to sign event: {e}"))))?;

	// Generate event id
	let event_id = gen_event_id(&signed_event, &body.room_version)?;

	// Add event_id back
	signed_event.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.to_string()));

	let sender_user: &UserId = signed_event
		.get("sender")
		.try_into()
		.map_err(|e| err!(Request(InvalidParam("Invalid sender property: {e}"))))?;

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
		.user_may_invite(sender_user.to_owned(), recipient_user.clone(), body.room_id.clone())
		.await
	{
		warn!("Antispam rejected invite: {e:?}");
		return Err!(Request(Forbidden("Invite rejected by antispam service.")));
	}

	let mut invite_state = body.invite_room_state.clone();

	let mut event: JsonObject = serde_json::from_str(body.event.get())
		.map_err(|e| err!(Request(BadJson("Invalid invite event PDU: {e}"))))?;

	event.insert("event_id".to_owned(), "$placeholder".into());

	let pdu: PduEvent = serde_json::from_value(event.into())
		.map_err(|e| err!(Request(BadJson("Invalid invite event PDU: {e}"))))?;

	invite_state.push(pdu.to_format());

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
				let request = ruma::api::appservice::event::push_events::v1::Request {
					events: vec![pdu.to_format()],
					txn_id: general_purpose::URL_SAFE_NO_PAD
						.encode(sha256::hash(pdu.event_id.as_bytes()))
						.into(),
					ephemeral: Vec::new(),
					to_device: Vec::new(),
				};
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

	Ok(create_invite::v2::Response {
		event: services
			.sending
			.convert_to_outgoing_federation_event(signed_event)
			.await,
	})
}
