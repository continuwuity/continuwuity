use std::{borrow::Borrow, iter::once};

use axum::extract::State;
use conduwuit::{Err, Result, at, err, info};
use futures::{StreamExt, TryStreamExt};
use ruma::{OwnedEventId, api::federation::event::get_room_state_ids};

use super::AccessCheck;
use crate::Ruma;

/// # `GET /_matrix/federation/v1/state_ids/{roomId}`
///
/// Retrieves a snapshot of a room's state at a given event, in the form of
/// event IDs.
pub(crate) async fn get_room_state_ids_route(
	State(services): State<crate::State>,
	body: Ruma<get_room_state_ids::v1::Request>,
) -> Result<get_room_state_ids::v1::Response> {
	AccessCheck {
		services: &services,
		origin: body.origin(),
		room_id: &body.room_id,
		event_id: None,
	}
	.check()
	.await?;

	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await
	{
		info!(
			origin = body.origin().as_str(),
			"Refusing to serve state for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	let shortstatehash = services
		.rooms
		.state_accessor
		.pdu_shortstatehash(&body.event_id)
		.await
		.map_err(|_| err!(Request(NotFound("Pdu state not found."))))?;

	let pdu_ids: Vec<OwnedEventId> = services
		.rooms
		.state_accessor
		.state_full_ids(shortstatehash)
		.map(at!(1))
		.collect()
		.await;

	let auth_chain_ids = services
		.rooms
		.auth_chain
		.event_ids_iter(&body.room_id, once(body.event_id.borrow()))
		.try_collect()
		.await?;

	Ok(get_room_state_ids::v1::Response { auth_chain_ids, pdu_ids })
}
