use axum::extract::State;
use conduwuit::{Err, Error, Result, debug_warn, info, matrix::pdu::PartialPdu, utils, warn};
use ruma::{
	api::{
		error::{ErrorKind, IncompatibleRoomVersionErrorData},
		federation::membership::prepare_knock_event,
	},
	events::room::member::{MembershipState, RoomMemberEventContent},
};
use serde_json::value::to_raw_value;

use crate::Ruma;

/// # `GET /_matrix/federation/v1/make_knock/{roomId}/{userId}`
///
/// Creates a knock template.
pub(crate) async fn create_knock_event_template_route(
	State(services): State<crate::State>,
	body: Ruma<prepare_knock_event::v1::Request>,
) -> Result<prepare_knock_event::v1::Response> {
	if !services.rooms.metadata.exists(&body.room_id).await {
		return Err!(Request(NotFound("Room is unknown to this server.")));
	}
	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await
	{
		info!(
			origin = body.origin().as_str(),
			room_id = %body.room_id,
			"Refusing to serve make_knock for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	if body.user_id.server_name() != body.origin() {
		return Err!(Request(BadJson("Not allowed to knock on behalf of another server/user.")));
	}

	// ACL check origin server
	services
		.rooms
		.event_handler
		.acl_check(body.origin(), &body.room_id)
		.await?;

	if services
		.moderation
		.is_remote_server_forbidden(body.origin())
	{
		warn!(
			"Server {} for remote user {} tried knocking room ID {} which has a server name \
			 that is globally forbidden. Rejecting.",
			body.origin(),
			&body.user_id,
			&body.room_id,
		);
		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			return Err!(Request(Forbidden("Server is banned on this homeserver.")));
		}
	}

	let room_version = services.rooms.state.get_room_version(&body.room_id).await?;
	let room_version_rules = room_version.rules().unwrap();

	if !room_version_rules.authorization.knocking {
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData::new(
				room_version,
			)),
			"Room version does not support knocking.",
		));
	}

	if !body.ver.contains(&room_version) {
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData::new(
				room_version,
			)),
			"Your homeserver does not support the features required to knock on this room.",
		));
	}

	let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	if let Ok(membership) = services
		.rooms
		.state_accessor
		.get_member(&body.room_id, &body.user_id)
		.await
	{
		if membership.membership == MembershipState::Ban {
			debug_warn!(
				"Remote user {} is banned from {} but attempted to knock",
				&body.user_id,
				&body.room_id
			);
			return Err!(Request(Forbidden("You cannot knock on a room you are banned from.")));
		}
	}

	let (pdu, _) = services
		.rooms
		.timeline
		.create_event(
			PartialPdu::state(
				body.user_id.to_string(),
				&RoomMemberEventContent::new(MembershipState::Knock),
			),
			&body.user_id,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);
	let mut pdu_json = utils::to_canonical_object(&pdu)
		.expect("Barebones PDU should be convertible to canonical JSON");
	pdu_json.remove("event_id");

	Ok(prepare_knock_event::v1::Response::new(
		room_version,
		to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON"),
	))
}
