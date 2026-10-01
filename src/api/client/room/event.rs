use axum::extract::{Query, State};
use conduwuit::{Err, Error, Event, PduEvent, Result, debug_warn, err};
use futures::TryFutureExt;
use http::StatusCode;
use ruma::{
	UserId,
	api::{client::room::get_room_event, error::ErrorKind},
};
use serde::Deserialize;
use serde_json::json;

use crate::{Ruma, client::is_ignored_pdu};

#[derive(Deserialize)]
pub(crate) struct Msc2815Params {
	#[serde(rename = "fi.mau.msc2815.include_unredacted_content", default)]
	include_unredacted_content: Option<bool>,
}

/// # `GET /_matrix/client/r0/rooms/{roomId}/event/{eventId}`
///
/// Gets a single event.
pub(crate) async fn get_room_event_route(
	State(ref services): State<crate::State>,
	Query(params): Query<Msc2815Params>,
	ref body: Ruma<get_room_event::v3::Request>,
) -> Result<get_room_event::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	let event_id = &body.event_id;
	let room_id = &body.room_id;

	let mut event = match services.rooms.timeline.get_pdu(event_id).await {
		| Ok(event) => event,
		| Err(e) if e.is_not_found() => {
			// Only fetch over federation for users who could see the event.
			let (joined, world_readable) = tokio::join!(
				services.rooms.state_cache.is_joined(sender_user, room_id),
				services.rooms.state_accessor.is_world_readable(room_id)
			);
			if !joined && !world_readable {
				return Err!(Request(NotFound("Event {event_id} not found.")));
			}

			services
				.rooms
				.timeline
				.get_remote_pdu(room_id, event_id)
				.await
				.map_err(|_| err!(Request(NotFound("Event {event_id} not found."))))?
		},
		| Err(e) => return Err(e),
	};
	if !event.room_id_or_hash().eq(room_id) {
		// Return not found instead of invalid param to avoid revealing the
		// existence of events the user might not be allowed to know about.
		return Err!(Request(NotFound("Event {event_id} not found.")));
	}

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

	if services.config.allow_moderators_to_view_redacted_content
		&& params.include_unredacted_content.unwrap_or(false)
		&& event.is_redacted()
	{
		unredact(services, sender_user, &mut event).await?;
	}

	if let Err(e) = services
		.rooms
		.pdu_metadata
		.add_bundled_aggregations_to_pdu(sender_user, &mut event)
		.await
	{
		debug_warn!("Failed to add bundled aggregations to event: {e}");
	}

	let ctx = services
		.rooms
		.timeline
		.get_unsigned_context(&event, Some(sender_user))
		.await;
	event.set_unsigned(Some(ctx))?;

	Ok(get_room_event::v3::Response::new(event.into_format()))
}

async fn unredact(services: &crate::State, sender_user: &UserId, event: &mut PduEvent) -> Result {
	if !services.users.is_admin(sender_user).await {
		let power_levels = services
			.rooms
			.state_accessor
			.get_room_power_levels(&event.room_id_or_hash())
			.await;

		if power_levels.for_user(sender_user) < power_levels.redact {
			return Err!(Request(Forbidden(
				"You don't have permission to view redacted content in this room."
			)));
		}
	}

	let content = services
		.rooms
		.timeline
		.get_pdu_id(event.event_id())
		.and_then(async |pdu_id| {
			services
				.rooms
				.timeline
				.get_unredacted_content(&pdu_id)
				.await
		})
		.await;

	let Ok(content) = content else {
		// CustomErrorKind has private fields; only serde can build one.
		let keep_ms = services
			.config
			.unredacted_content_max_age_secs
			.saturating_mul(1000);

		let kind = serde_json::from_value::<ErrorKind>(json!({
			"errcode": "FI.MAU.MSC2815_UNREDACTED_CONTENT_DELETED",
			"fi.mau.msc2815.content_keep_ms": keep_ms,
		}))
		.expect("errcode is a valid custom error kind");

		return Err(Error::Request(
			kind,
			"The unredacted content of this event is not available.".into(),
			StatusCode::NOT_FOUND,
		));
	};

	event.content = content;

	Ok(())
}
