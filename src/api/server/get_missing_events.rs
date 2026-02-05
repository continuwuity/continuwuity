use axum::extract::State;
use conduwuit::{Err, Result, debug, debug_error, info, utils::to_canonical_object};
use ruma::api::federation::event::get_missing_events;

use super::AccessCheck;
use crate::Ruma;

/// arbitrary number but synapse's is 20 and we can handle lots of these anyways
const LIMIT_MAX: usize = 50;
/// spec says default is 10
const LIMIT_DEFAULT: usize = 10;

/// # `POST /_matrix/federation/v1/get_missing_events/{roomId}`
///
/// Retrieves events that the sender is missing.
pub(crate) async fn get_missing_events_route(
	State(services): State<crate::State>,
	body: Ruma<get_missing_events::v1::Request>,
) -> Result<get_missing_events::v1::Response> {
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

	let limit = body
		.limit
		.try_into()
		.unwrap_or(LIMIT_DEFAULT)
		.min(LIMIT_MAX);

	let mut queued_events = body.latest_events.clone();
	// the vec will never have more entries the limit
	let mut events = Vec::with_capacity(limit);

	let mut i: usize = 0;
	while i < queued_events.len() && events.len() < limit {
		let Ok(pdu) = services.rooms.timeline.get_pdu(&queued_events[i]).await else {
			debug!(
				body.origin = body.origin.as_ref().map(tracing::field::display),
				"Event {} does not exist locally, skipping", &queued_events[i]
			);
			i = i.saturating_add(1);
			continue;
		};

		if body.earliest_events.contains(&queued_events[i]) {
			i = i.saturating_add(1);
			continue;
		}

		if !services
			.rooms
			.state_accessor
			.server_can_see_event(body.origin(), &body.room_id, &queued_events[i])
			.await
		{
			debug!(
				body.origin = body.origin.as_ref().map(tracing::field::display),
				"Server cannot see {:?} in {:?}, skipping", pdu.event_id, pdu.room_id
			);
			i = i.saturating_add(1);
			continue;
		}

		i = i.saturating_add(1);
		let Ok(event) = to_canonical_object(&pdu) else {
			debug_error!(
				body.origin = body.origin.as_ref().map(tracing::field::display),
				"Failed to convert PDU in database to canonical JSON: {pdu:?}"
			);
			continue;
		};

		let prev_events = pdu.prev_events.iter().map(ToOwned::to_owned);

		let event = services
			.sending
			.convert_to_outgoing_federation_event(event)
			.await;

		queued_events.extend(prev_events);
		events.push(event);
	}

	Ok(get_missing_events::v1::Response { events })
}
