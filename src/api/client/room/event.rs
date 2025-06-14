use axum::extract::State;
use conduwuit::{Err, Event, Result, debug_warn, err};
use futures::{FutureExt, TryFutureExt, future::try_join};
use ruma::api::client::room::get_room_event;

use crate::{Ruma, client::is_ignored_pdu};

/// # `GET /_matrix/client/r0/rooms/{roomId}/event/{eventId}`
///
/// Gets a single event.
pub(crate) async fn get_room_event_route(
	State(ref services): State<crate::State>,
	ref body: Ruma<get_room_event::v3::Request>,
) -> Result<get_room_event::v3::Response> {
	let event_id = &body.event_id;
	let room_id = &body.room_id;

	let event = services
		.rooms
		.timeline
		.get_pdu(event_id)
		.map_err(|_| err!(Request(NotFound("Event {} not found.", event_id))));

	let visible = services
		.rooms
		.state_accessor
		.user_can_see_event(body.sender_user(), room_id, event_id)
		.map(Ok);

	let (mut event, visible) = try_join(event, visible).await?;

	if !visible || is_ignored_pdu(services, &event, body.sender_user()).await {
		return Err!(Request(Forbidden("You don't have permission to view this event.")));
	}

	debug_assert!(
		event.event_id() == event_id && event.room_id() == room_id,
		"Fetched PDU must match requested"
	);

	if let Err(e) = services
		.rooms
		.pdu_metadata
		.add_bundled_aggregations_to_pdu(body.sender_user(), &mut event)
		.await
	{
		debug_warn!("Failed to add bundled aggregations to event: {e}");
	}

	event.set_unsigned(body.sender_user.as_deref());

	Ok(get_room_event::v3::Response { event: event.into_room_event() })
}
