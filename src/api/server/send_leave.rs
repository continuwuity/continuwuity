use axum::extract::State;
use conduwuit::{Err, Result, debug_info, err};
use futures::FutureExt;
use ruma::{
	api::federation::membership::create_leave_event, events::room::member::MembershipState,
};

use crate::{Ruma, server::utils::validate_any_membership_event};

/// # `PUT /_matrix/federation/v2/send_leave/{roomId}/{eventId}`
///
/// Submits a signed leave event.
pub(crate) async fn create_leave_event_v2_route(
	State(services): State<crate::State>,
	body: Ruma<create_leave_event::v2::Request>,
) -> Result<create_leave_event::v2::Response> {
	let room_id = body.room_id.as_ref();
	let origin = &body.identity;
	if !services.rooms.metadata.exists(room_id).await {
		return Err!(Request(NotFound("Room is unknown to this server.")));
	}

	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), room_id)
		.await
	{
		debug_info!(
			origin = origin.as_str(),
			room_id = %room_id,
			"Refusing to send_leave for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	// ACL check origin
	services
		.rooms
		.event_handler
		.acl_check(origin, room_id)
		.await?;

	let room_version = services.rooms.state.get_room_version(room_id).await?;
	let create_event = services
		.rooms
		.state_accessor
		.get_room_create_event(room_id)
		.await;
	let room_version_rules = room_version.rules().unwrap();

	let (value, membership, sender, target) = validate_any_membership_event(
		&services,
		&body.pdu,
		&room_version_rules,
		create_event.event_id.clone(),
		body.room_id.clone(),
		body.event_id.clone(),
	)
	.await?;
	if membership != MembershipState::Leave {
		return Err!(Request(InvalidParam("Invalid membership (expected `leave`)")));
	}
	if sender.server_name() != body.identity {
		return Err!(Request(InvalidParam("Sender belongs to a different server")));
	}
	if sender != target {
		return Err!(Request(InvalidParam("Sender does not match state key")));
	}

	let mutex_lock = services
		.rooms
		.event_handler
		.mutex_federation
		.lock(room_id)
		.await;

	let pdu_id = services
		.rooms
		.event_handler
		.handle_incoming_pdu(origin, room_id, &body.event_id, value, false)
		.boxed()
		.await?
		.ok_or_else(|| err!(Request(InvalidParam("Could not accept as timeline event."))))?;

	drop(mutex_lock);

	services
		.sending
		.send_pdu_room(room_id, &pdu_id)
		.boxed()
		.await?;

	Ok(create_leave_event::v2::Response::new())
}
