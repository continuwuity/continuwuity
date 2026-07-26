#[cfg(test)]
mod tests;
use axum::extract::State;
use conduwuit::{Err, Result, err, matrix::Event};
use futures::{FutureExt, TryStreamExt};
use ruma::{
	RoomId,
	api::client::state::{
		get_state_event_for_key::{self, v3::StateEventFormat},
		get_state_events, send_state_event,
	},
};
use serde_json::{json, value::to_raw_value};

use crate::{Ruma, RumaResponse, client_ip::ClientIp};

/// # `PUT /_matrix/client/*/rooms/{roomId}/state/{eventType}/{stateKey}`
///
/// Sends a state event into the room.
pub(crate) async fn send_state_event_for_key_route(
	State(services): State<crate::State>,
	ClientIp(ip): ClientIp, // NOTE: Required for updating device metadata
	body: Ruma<send_state_event::v3::Request>,
) -> Result<send_state_event::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	services
		.users
		.update_device_last_seen(sender_user, body.identity.sender_device(), ip)
		.await;

	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let state_lock = services
		.rooms
		.state
		.mutex
		.lock::<RoomId>(&body.room_id)
		.await;

	let event_id = services
		.rooms
		.timeline
		.send_state_event_for_key(
			sender_user,
			&body.room_id,
			&state_lock,
			&body.event_type,
			&body.body.body,
			&body.state_key,
			if body.identity.is_appservice() {
				body.timestamp
			} else {
				None
			},
		)
		.boxed()
		.await?;

	Ok(send_state_event::v3::Response::new(event_id))
}

/// # `PUT /_matrix/client/*/rooms/{roomId}/state/{eventType}`
///
/// Sends a state event into the room.
pub(crate) async fn send_state_event_for_empty_key_route(
	State(services): State<crate::State>,
	ClientIp(ip): ClientIp, // NOTE: Required for updating device metadata
	body: Ruma<send_state_event::v3::Request>,
) -> Result<RumaResponse<send_state_event::v3::Response>> {
	send_state_event_for_key_route(State(services), ClientIp(ip), body)
		.boxed()
		.await
		.map(RumaResponse)
}

/// # `GET /_matrix/client/v3/rooms/{roomid}/state`
///
/// Get all state events for a room.
///
/// - If not joined: Only works if current room history visibility is world
///   readable
pub(crate) async fn get_state_events_route(
	State(services): State<crate::State>,
	body: Ruma<get_state_events::v3::Request>,
) -> Result<get_state_events::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;

	if !services
		.rooms
		.state_accessor
		.user_can_see_state_events(sender_user, &body.room_id)
		.await
	{
		return Err!(Request(Forbidden("You don't have permission to view the room state.")));
	}

	let room_state = services
		.rooms
		.state_accessor
		.room_state_full_pdus(&body.room_id)
		.map_ok(Event::into_format)
		.try_collect()
		.await?;

	Ok(get_state_events::v3::Response::new(room_state))
}

/// # `GET /_matrix/client/v3/rooms/{roomid}/state/{eventType}/{stateKey}`
///
/// Get single state event of a room with the specified state key.
/// The optional query parameter `?format=event|content` allows returning the
/// full room state event or just the state event's content (default behaviour)
///
/// - If not joined: Only works if current room history visibility is world
///   readable
pub(crate) async fn get_state_event_for_key_route(
	State(services): State<crate::State>,
	body: Ruma<get_state_event_for_key::v3::Request>,
) -> Result<get_state_event_for_key::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;

	if !services
		.rooms
		.state_accessor
		.user_can_see_state_events(sender_user, &body.room_id)
		.await
	{
		return Err!(Request(NotFound(debug_warn!(
			"You don't have permission to view the room state."
		))));
	}

	let event = services
		.rooms
		.state_accessor
		.room_state_get(&body.room_id, &body.event_type, &body.state_key)
		.await
		.map_err(|_| {
			err!(Request(NotFound(debug_warn!(
					room_id = %body.room_id,
					event_type = %body.event_type,
					"State event not found in room.",
			))))
		})?;

	let content = match body.format {
		| StateEventFormat::Content => event.content,
		| StateEventFormat::Event => to_raw_value(&json!({
			"content": event.content(),
			"event_id": event.event_id(),
			"origin_server_ts": event.origin_server_ts(),
			"room_id": event.room_id_or_hash(),
			"sender": event.sender(),
			"state_key": event.state_key(),
			"type": event.kind(),
			"unsigned": event.unsigned(),
		}))
		.unwrap(),
		| _ => return Err!(Request(InvalidParam("Unknown response format"))),
	};

	Ok(get_state_event_for_key::v3::Response::new(content))
}

/// # `GET /_matrix/client/v3/rooms/{roomid}/state/{eventType}`
///
/// Get single state event of a room.
/// The optional query parameter `?format=event|content` allows returning the
/// full room state event or just the state event's content (default behaviour)
///
/// - If not joined: Only works if current room history visibility is world
///   readable
pub(crate) async fn get_state_events_for_empty_key_route(
	State(services): State<crate::State>,
	body: Ruma<get_state_event_for_key::v3::Request>,
) -> Result<RumaResponse<get_state_event_for_key::v3::Response>> {
	get_state_event_for_key_route(State(services), body)
		.await
		.map(RumaResponse)
}
