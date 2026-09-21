use axum::extract::State;
use conduwuit::{Err, Event, Result, debug_warn, err};
use ruma::api::client::room::get_room_event;

use crate::{Ruma, client::is_ignored_pdu};

/// # `GET /_matrix/client/r0/rooms/{roomId}/event/{eventId}`
///
/// Gets a single event.
pub(crate) async fn get_room_event_route(
	State(ref services): State<crate::State>,
	ref body: Ruma<get_room_event::v3::Request>,
) -> Result<get_room_event::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	let event_id = &body.event_id;
	let room_id = &body.room_id;

	let mut event = match services.rooms.timeline.get_pdu(event_id).await {
		| Ok(event) => event,
		| Err(_) => {
			// Only fetch over federation for users who could see the event.
			if !services
				.rooms
				.state_cache
				.is_joined(sender_user, room_id)
				.await && !services
				.rooms
				.state_accessor
				.is_world_readable(room_id)
				.await
			{
				return Err!(Request(NotFound("Event {} not found.", event_id)));
			}

			services
				.rooms
				.timeline
				.get_remote_pdu(room_id, event_id)
				.await
				.map_err(|_| err!(Request(NotFound("Event {} not found.", event_id))))?
		},
	};

	// NOTE: checked after fetching, as visibility of an unknown event cannot
	// be determined (user_can_see_event returns true for those).
	let visible = services
		.rooms
		.state_accessor
		.user_can_see_event(sender_user, room_id, event_id)
		.await;

	if !visible || is_ignored_pdu(services, &event, sender_user).await? {
		return Err!(Request(Forbidden("You don't have permission to view this event.")));
	}

	if let Err(e) = services
		.rooms
		.pdu_metadata
		.add_bundled_aggregations_to_pdu(sender_user, &mut event)
		.await
	{
		debug_warn!("Failed to add bundled aggregations to event: {e}");
	}

	event.set_unsigned(Some(body.identity.expect_sender_user()?));

	Ok(get_room_event::v3::Response::new(event.into_format()))
}
