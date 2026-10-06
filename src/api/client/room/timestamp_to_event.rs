use axum::extract::State;
use conduwuit::{Err, Event, Result, utils::stream::TryIgnore};
use futures::{StreamExt, pin_mut};
use ruma::api::client::room::get_event_by_timestamp;

use crate::Ruma;

const MAX_CANDIDATES: usize = 20;

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

	if !services.rooms.timeline.timestamp_index_ready().await {
		return Err!(FeatureDisabled("timestamp_to_event"));
	}

	let candidates = services
		.rooms
		.timeline
		.pdus_by_timestamp(room_id, body.ts, body.dir)
		.ignore_err()
		.take(MAX_CANDIDATES);

	pin_mut!(candidates);
	while let Some((_, pdu)) = candidates.next().await {
		if services
			.rooms
			.state_accessor
			.user_can_see_event(sender_user, room_id, pdu.event_id())
			.await
		{
			return Ok(get_event_by_timestamp::v1::Response::new(
				pdu.event_id().to_owned(),
				pdu.origin_server_ts(),
			));
		}
	}

	Err!(Request(NotFound(
		"Unable to find event from {} in direction {:?}",
		body.ts.get(),
		body.dir
	)))
}
