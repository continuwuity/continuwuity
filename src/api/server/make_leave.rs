use axum::extract::State;
use conduwuit::{Err, Result, info, matrix::pdu::PartialPdu, utils};
use ruma::{
	api::federation::membership::prepare_leave_event,
	events::room::member::{MembershipState, RoomMemberEventContent},
};
use serde_json::value::to_raw_value;

use crate::Ruma;

/// # `GET /_matrix/federation/v1/make_leave/{roomId}/{eventId}`
///
/// Creates a leave template.
pub(crate) async fn create_leave_event_template_route(
	State(services): State<crate::State>,
	body: Ruma<prepare_leave_event::v1::Request>,
) -> Result<prepare_leave_event::v1::Response> {
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
			"Refusing to serve make_leave for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	if body.user_id.server_name() != body.origin() {
		return Err!(Request(Forbidden(
			"Not allowed to leave on behalf of another server/user."
		)));
	}

	// ACL check origin
	services
		.rooms
		.event_handler
		.acl_check(body.origin(), &body.room_id)
		.await?;

	let room_version = services.rooms.state.get_room_version(&body.room_id).await?;
	let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	let (pdu, _) = services
		.rooms
		.timeline
		.create_event(
			PartialPdu::state(
				body.user_id.to_string(),
				&RoomMemberEventContent::new(MembershipState::Leave),
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

	Ok(prepare_leave_event::v1::Response::new(
		Some(room_version),
		to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON"),
	))
}
