use axum::extract::State;
use conduwuit::{Err, Event, Result, err};
use ruma::api::{Direction, client::room::get_event_by_timestamp};

use crate::Ruma;

/// # `GET /_matrix/client/v1/rooms/{roomId}/timestamp_to_event`
///
/// Gets the ID of the event closest to the given timestamp, in the direction
/// specified by `dir`.
pub(crate) async fn get_event_by_timestamp_route(
	State(ref services): State<crate::State>,
	body: Ruma<get_event_by_timestamp::v1::Request>,
) -> Result<get_event_by_timestamp::v1::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	let room_id = &body.room_id;

	if !services
		.rooms
		.state_accessor
		.user_can_see_state_events(sender_user, room_id)
		.await
	{
		return Err!(Request(Forbidden("You don't have permission to view this room.")));
	}

	let dir = match body.dir {
		| Direction::Forward => "f",
		| Direction::Backward => "b",
	};

	let not_found = || {
		err!(Request(NotFound(
			"Unable to find event from {} in direction {}",
			body.ts.get(),
			dir
		)))
	};

	let Some((_, event)) = services
		.rooms
		.timeline
		.event_by_timestamp(room_id, body.ts, body.dir)
		.await
	else {
		return Err(not_found());
	};

	if !services
		.rooms
		.state_accessor
		.user_can_see_event(sender_user, room_id, event.event_id())
		.await
	{
		return Err!(Request(Forbidden("You don't have permission to view this event.")));
	}

	Ok(get_event_by_timestamp::v1::Response::new(
		event.event_id().to_owned(),
		event.origin_server_ts(),
	))
}
