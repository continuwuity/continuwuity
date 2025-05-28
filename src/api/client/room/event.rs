use axum::extract::State;
use conduwuit::{Err, Event, PduEvent, Result, err};
use futures::{FutureExt, TryFutureExt, future::try_join};
use ruma::api::client::{error::ErrorKind, room::get_room_event};

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
	let sender_user = body.sender_user();

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

	let include_unredacted_content = body
		.include_unredacted_content // User's file has this field name
		.unwrap_or(false);

	if include_unredacted_content && event.is_redacted() {
		let is_server_admin = services
			.users
			.is_admin(sender_user)
			.map(|is_admin| Ok(is_admin));
		let can_redact_privilege = services
			.rooms
			.state_accessor
			.user_can_redact(event_id, sender_user, room_id, false) // federation=false for local check
			;
		let (is_server_admin, can_redact_privilege) =
			try_join(is_server_admin, can_redact_privilege).await?;

		if !is_server_admin && !can_redact_privilege {
			return Err!(Request(Forbidden(
				"You don't have permission to view redacted content.",
			)));
		}

		let pdu_id = match services.rooms.timeline.get_pdu_id(event_id).await {
			| Ok(id) => id,
			| Err(e) => {
				return Err(e);
			},
		};
		let original_content = services
			.rooms
			.timeline
			.get_original_pdu_content(&pdu_id)
			.await?;
		if let Some(original_content) = original_content {
			// If the original content is available, we can return it.
			// event.content = to_raw_value(&original_content)?;
			event = PduEvent::from_id_val(event_id, original_content)?;
		} else {
			return Err(conduwuit::Error::BadRequest(
				ErrorKind::UnredactedContentDeleted { content_keep_ms: None },
				"The original unredacted content is not in the database.",
			));
		}
	}

	debug_assert!(
		event.event_id() == event_id && event.room_id() == room_id,
		"Fetched PDU must match requested"
	);

	event.add_age().ok();

	Ok(get_room_event::v3::Response { event: event.into_room_event() })
}
