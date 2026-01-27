use axum::extract::State;
use conduwuit::{
	Err, Result, err, info,
	matrix::{event::gen_event_id_canonical_json, pdu::PduEvent},
	warn,
};
use futures::FutureExt;
use ruma::{
	OwnedUserId,
	RoomVersionId::*,
	api::federation::knock::send_knock,
	events::{
		StateEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
	serde::JsonObject,
};

use crate::Ruma;

/// # `PUT /_matrix/federation/v1/send_knock/{roomId}/{eventId}`
///
/// Submits a signed knock event.
pub(crate) async fn create_knock_event_v1_route(
	State(services): State<crate::State>,
	body: Ruma<send_knock::v1::Request>,
) -> Result<send_knock::v1::Response> {
	if services
		.moderation
		.is_remote_server_forbidden(body.origin())
	{
		warn!(
			"Server {} tried knocking room ID {} who has a server name that is globally \
			 forbidden. Rejecting.",
			body.origin(),
			&body.room_id,
		);
		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			warn!(
				"Server {} tried knocking room ID {} which has a server name that is globally \
				 forbidden. Rejecting.",
				body.origin(),
				&body.room_id,
			);
			return Err!(Request(Forbidden("Server is banned on this homeserver.")));
		}
	}

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
			"Refusing to serve send_knock for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	// ACL check origin server
	services
		.rooms
		.event_handler
		.acl_check(body.origin(), &body.room_id)
		.await?;

	let room_version_id = services.rooms.state.get_room_version(&body.room_id).await?;

	if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6) {
		return Err!(Request(Forbidden("Room version does not support knocking.")));
	}

	let Ok((event_id, value)) = gen_event_id_canonical_json(&body.pdu, &room_version_id) else {
		// Event could not be converted to canonical json
		return Err!(Request(InvalidParam("Could not convert event to canonical json.")));
	};

	let event_type: StateEventType = serde_json::from_value(
		value
			.get("type")
			.ok_or_else(|| err!(Request(InvalidParam("Event has no event type."))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(InvalidParam("Event has invalid event type: {e}"))))?;

	if event_type != StateEventType::RoomMember {
		return Err!(Request(InvalidParam(
			"Not allowed to send non-membership state event to knock endpoint.",
		)));
	}

	let content: RoomMemberEventContent = serde_json::from_value(
		value
			.get("content")
			.ok_or_else(|| err!(Request(InvalidParam("Membership event has no content"))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(InvalidParam("Event has invalid membership content: {e}"))))?;

	if content.membership != MembershipState::Knock {
		return Err!(Request(InvalidParam(
			"Not allowed to send a non-knock membership event to knock endpoint."
		)));
	}

	// ACL check sender server name
	let sender: OwnedUserId = serde_json::from_value(
		value
			.get("sender")
			.ok_or_else(|| err!(Request(InvalidParam("Event has no sender user ID."))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson("Event sender is not a valid user ID: {e}"))))?;

	services
		.rooms
		.event_handler
		.acl_check(sender.server_name(), &body.room_id)
		.await?;

	// check if origin server is trying to send for another server
	if sender.server_name() != body.origin() {
		return Err!(Request(BadJson("Not allowed to knock on behalf of another server/user.")));
	}

	let state_key: OwnedUserId = serde_json::from_value(
		value
			.get("state_key")
			.ok_or_else(|| err!(Request(InvalidParam("Event does not have a state_key"))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson("Event does not have a valid state_key: {e}"))))?;

	if state_key != sender {
		return Err!(Request(InvalidParam("state_key does not match sender user of event.")));
	}

	let mut event: JsonObject = serde_json::from_str(body.pdu.get())
		.map_err(|e| err!(Request(InvalidParam("Invalid knock event PDU: {e}"))))?;

	event.insert("event_id".to_owned(), "$placeholder".into());

	let pdu: PduEvent = serde_json::from_value(event.into())
		.map_err(|e| err!(Request(InvalidParam("Invalid knock event PDU: {e}"))))?;

	let mutex_lock = services
		.rooms
		.event_handler
		.mutex_federation
		.lock(&body.room_id)
		.await;

	let pdu_id = services
		.rooms
		.event_handler
		.handle_incoming_pdu(sender.server_name(), &body.room_id, &event_id, value.clone(), true)
		.boxed()
		.await?
		.ok_or_else(|| err!(Request(InvalidParam("Could not accept as timeline event."))))?;

	drop(mutex_lock);

	services
		.sending
		.send_pdu_room(&body.room_id, &pdu_id)
		.await?;

	let knock_room_state = services
		.rooms
		.state
		.summary_stripped(&pdu, &body.room_id)
		.await;

	Ok(send_knock::v1::Response { knock_room_state })
}
