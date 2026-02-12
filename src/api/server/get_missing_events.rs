use std::collections::{HashSet, VecDeque};

use axum::extract::State;
use conduwuit::{Err, Event, Result, debug, info, trace, utils::to_canonical_object, warn};
use ruma::{OwnedEventId, api::federation::event::get_missing_events};
use serde_json::{json, value::RawValue};

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

	let room_version = services.rooms.state.get_room_version(&body.room_id).await?;

	let mut queue: VecDeque<OwnedEventId> = VecDeque::from(body.latest_events.clone());
	let mut results: Vec<Box<RawValue>> = Vec::with_capacity(limit);
	let mut seen: HashSet<OwnedEventId> = HashSet::from_iter(body.earliest_events.clone());

	while let Some(next_event_id) = queue.pop_front() {
		if seen.contains(&next_event_id) {
			trace!(%next_event_id, "already seen event, skipping");
			continue;
		}

		if results.len() >= limit {
			debug!(%next_event_id, "reached limit of events to return, breaking");
			break;
		}

		let mut pdu = match services.rooms.timeline.get_pdu(&next_event_id).await {
			| Ok(pdu) => pdu,
			| Err(e) => {
				warn!("could not find event {next_event_id} while walking missing events: {e}");
				continue;
			},
		};
		if pdu.room_id_or_hash() != body.room_id {
			return Err!(Request(Unknown(
				"Event {next_event_id} is not in room {}",
				body.room_id
			)));
		}

		if !services
			.rooms
			.state_accessor
			.server_can_see_event(body.origin(), &body.room_id, pdu.event_id())
			.await
		{
			debug!(%next_event_id, origin = %body.origin(), "redacting event origin cannot see");
			pdu.redact(&room_version, json!({}))?
		}

		trace!(
			%next_event_id,
			prev_events = ?pdu.prev_events().collect::<Vec<_>>(),
			"adding event to results and queueing prev events"
		);
		queue.extend(pdu.prev_events.clone());
		seen.insert(next_event_id.clone());
		if body.latest_events.contains(&next_event_id) {
			continue; // Don't include latest_events in results,
			// but do include their prev_events in the queue
		}
		results.push(
			services
				.sending
				.convert_to_outgoing_federation_event(to_canonical_object(pdu)?)
				.await,
		);
		trace!(
			%next_event_id,
			queue_len = queue.len(),
			seen_len = seen.len(),
			results_len = results.len(),
			"event added to results"
		)
	}

	if !queue.is_empty() {
		debug!("limit reached before queue was empty");
	}
	results.reverse(); // return oldest first
	Ok(get_missing_events::v1::Response { events: results })
}
