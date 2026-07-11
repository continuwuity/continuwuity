use axum::extract::State;
use conduwuit::{Err, Event, Result, debug_info, err, matrix::pdu::PduEvent, warn};
use futures::FutureExt;
use ruma::{
	api::federation::membership::create_knock_event, events::room::member::MembershipState,
};

use crate::{Ruma, server::utils::validate_any_membership_event};

/// # `PUT /_matrix/federation/v1/send_knock/{roomId}/{eventId}`
///
/// Submits a signed knock event.
pub(crate) async fn create_knock_event_v1_route(
	State(services): State<crate::State>,
	body: Ruma<create_knock_event::v1::Request>,
) -> Result<create_knock_event::v1::Response> {
	if services
		.moderation
		.is_remote_server_forbidden(&body.identity)
	{
		warn!(
			"Server {} tried knocking room ID {} who has a server name that is globally \
			 forbidden. Rejecting.",
			body.identity, &body.room_id,
		);
		return Err!(Request(Forbidden("Federation denied with {}", body.identity)));
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
		debug_info!(
			origin = body.identity.as_str(),
			room_id = %body.room_id,
			"Refusing to serve send_knock for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	// ACL check origin server
	services
		.rooms
		.event_handler
		.acl_check(&body.identity, &body.room_id)
		.await?;

	let room_version = services.rooms.state.get_room_version(&body.room_id).await?;
	let create_event = services
		.rooms
		.state_accessor
		.get_room_create_event(&body.room_id)
		.await;
	let room_version_rules = room_version.rules().unwrap();

	if !room_version_rules.authorization.knocking {
		return Err!(Request(Forbidden("Room version does not support knocking.")));
	}

	let (mut event, target_membership, sender, target) = validate_any_membership_event(
		&services,
		&body.pdu,
		&room_version_rules,
		create_event.event_id().to_owned(),
		body.room_id.clone(),
		body.event_id.clone(),
	)
	.await?;

	if target_membership != MembershipState::Knock {
		return Err!(Request(InvalidParam("Invalid membership (expected `knock`)")));
	}
	if sender.server_name() != body.identity {
		return Err!(Request(InvalidParam("Sender belongs to a different server")));
	}
	if sender != target {
		return Err!(Request(InvalidParam("Sender does not match state key")));
	}

	event.insert("event_id".to_owned(), body.event_id.as_str().into());

	let pdu = PduEvent::from_id_val(&body.event_id, event.clone())
		.map_err(|e| err!(Request(InvalidParam("Invalid knock event PDU: {e}"))))?;

	let mutex_lock = services
		.rooms
		.event_handler
		.mutex_federation
		.lock(body.room_id.as_str())
		.await;

	let pdu_id = services
		.rooms
		.event_handler
		.handle_incoming_pdu(sender.server_name(), &body.room_id, &body.event_id, event, false)
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
		.summary_stripped(&pdu, &body.room_id, &sender)
		.await;

	Ok(create_knock_event::v1::Response::new(knock_room_state))
}
