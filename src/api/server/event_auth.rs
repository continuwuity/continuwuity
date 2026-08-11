use std::{borrow::Borrow, iter::once};

use axum::extract::State;
use conduwuit::{Err, Event, Result, info, utils::stream::ReadyExt};
use futures::StreamExt;
use ruma::api::federation::authorization::get_event_authorization;

use super::AccessCheck;
use crate::Ruma;

/// # `GET /_matrix/federation/v1/event_auth/{roomId}/{eventId}`
///
/// Retrieves the auth chain for a given event.
///
/// - This does not include the event itself
pub(crate) async fn get_event_authorization_route(
	State(services): State<crate::State>,
	body: Ruma<get_event_authorization::v1::Request>,
) -> Result<get_event_authorization::v1::Response> {
	AccessCheck {
		services: &services,
		origin: &body.identity,
		room_id: &body.room_id,
		event_id: None,
	}
	.assert()
	.await?;

	if services
		.rooms
		.pdu_metadata
		.is_event_rejected(&body.event_id)
		.await
	{
		return Err!(Request(NotFound("Event not found.")));
	}

	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await
	{
		info!(
			origin = body.identity.as_str(),
			"Refusing to serve state for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	// The event must be in the room we just authorised access to
	if !services
		.rooms
		.timeline
		.get_pdu(&body.event_id)
		.await
		.is_ok_and(|pdu| pdu.room_id_or_hash() == body.room_id)
	{
		return Err!(Request(NotFound("Event not found.")));
	}

	let auth_chain = services
		.rooms
		.auth_chain
		.event_ids_iter(&body.room_id, once(body.event_id.borrow()))
		.ready_filter_map(Result::ok)
		.filter_map(|id| async move { services.rooms.timeline.get_pdu_json(&id).await.ok() })
		.then(|pdu| services.sending.convert_to_outgoing_federation_event(pdu))
		.collect()
		.await;

	Ok(get_event_authorization::v1::Response::new(auth_chain))
}
