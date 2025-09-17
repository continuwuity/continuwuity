use axum::extract::State;
use conduwuit::{
	Result, at,
	matrix::{Event, event::RelationTypeEqual, pdu::PduCount},
	utils::{IterStream, ReadyExt, result::FlatOk, stream::WidebandExt},
};
use conduwuit_service::Services;
use futures::StreamExt;
use ruma::{
	EventId, RoomId, UInt, UserId,
	api::{
		Direction,
		client::relations::{
			get_relating_events, get_relating_events_with_rel_type,
			get_relating_events_with_rel_type_and_event_type,
		},
	},
	events::{TimelineEventType, relation::RelationType},
};

use crate::Ruma;

/// # `GET /_matrix/client/r0/rooms/{roomId}/relations/{eventId}/{relType}/{eventType}`
pub(crate) async fn get_relating_events_with_rel_type_and_event_type_route(
	State(services): State<crate::State>,
	body: Ruma<get_relating_events_with_rel_type_and_event_type::v1::Request>,
) -> Result<get_relating_events_with_rel_type_and_event_type::v1::Response> {
	paginate_relations_with_filter(
		&services,
		body.sender_user(),
		&body.room_id,
		&body.event_id,
		body.event_type.clone().into(),
		body.rel_type.clone().into(),
		body.from.as_deref(),
		body.to.as_deref(),
		body.limit,
		body.recurse,
		body.dir,
	)
	.await
	.map(|res| get_relating_events_with_rel_type_and_event_type::v1::Response {
		chunk: res.chunk,
		next_batch: res.next_batch,
		prev_batch: res.prev_batch,
		recursion_depth: res.recursion_depth,
	})
}

/// # `GET /_matrix/client/r0/rooms/{roomId}/relations/{eventId}/{relType}`
pub(crate) async fn get_relating_events_with_rel_type_route(
	State(services): State<crate::State>,
	body: Ruma<get_relating_events_with_rel_type::v1::Request>,
) -> Result<get_relating_events_with_rel_type::v1::Response> {
	paginate_relations_with_filter(
		&services,
		body.sender_user(),
		&body.room_id,
		&body.event_id,
		None,
		body.rel_type.clone().into(),
		body.from.as_deref(),
		body.to.as_deref(),
		body.limit,
		body.recurse,
		body.dir,
	)
	.await
	.map(|res| get_relating_events_with_rel_type::v1::Response {
		chunk: res.chunk,
		next_batch: res.next_batch,
		prev_batch: res.prev_batch,
		recursion_depth: res.recursion_depth,
	})
}

/// # `GET /_matrix/client/r0/rooms/{roomId}/relations/{eventId}`
pub(crate) async fn get_relating_events_route(
	State(services): State<crate::State>,
	body: Ruma<get_relating_events::v1::Request>,
) -> Result<get_relating_events::v1::Response> {
	paginate_relations_with_filter(
		&services,
		body.sender_user(),
		&body.room_id,
		&body.event_id,
		None,
		None,
		body.from.as_deref(),
		body.to.as_deref(),
		body.limit,
		body.recurse,
		body.dir,
	)
	.await
}

#[allow(clippy::too_many_arguments)]
async fn paginate_relations_with_filter(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	target: &EventId,
	filter_event_type: Option<TimelineEventType>,
	filter_rel_type: Option<RelationType>,
	from: Option<&str>,
	to: Option<&str>,
	limit: Option<UInt>,
	recurse: bool,
	dir: Direction,
) -> Result<get_relating_events::v1::Response> {
	let start: PduCount = from
		.map(str::parse)
		.transpose()?
		.unwrap_or_else(|| match dir {
			| Direction::Forward => PduCount::min(),
			| Direction::Backward => PduCount::max(),
		});

	let to: Option<PduCount> = to.map(str::parse).transpose()?;

	// Use limit or else 30, with maximum 100
	let limit: usize = limit
		.map(TryInto::try_into)
		.flat_ok()
		.unwrap_or(30)
		.min(100);

	// Spec (v1.10) recommends depth of at least 3
	let depth: u8 = if recurse { 3 } else { 1 };

	// Check if this is a thread request
	let is_thread = filter_rel_type
		.as_ref()
		.is_some_and(|rel| *rel == RelationType::Thread);

	let events: Vec<_> = services
		.rooms
		.pdu_metadata
		.get_relations(sender_user, room_id, target, start, limit, depth, dir)
		.await
		.into_iter()
		.filter(|(_, pdu)| {
			filter_event_type
				.as_ref()
				.is_none_or(|kind| kind == pdu.kind())
		})
		.filter(|(_, pdu)| {
			filter_rel_type
				.as_ref()
				.is_none_or(|rel_type| rel_type.relation_type_equal(pdu))
		})
		.stream()
		.ready_take_while(|(count, _)| Some(*count) != to)
		.wide_filter_map(|item| visibility_filter(services, sender_user, item))
		.take(limit)
		.collect()
		.await;

	// For threads, check if we should include the root event
	let mut root_event = None;
	if is_thread && dir == Direction::Backward {
		// Check if we've reached the beginning of the thread
		// (fewer events than requested means we've exhausted the thread)
		if events.len() < limit {
			// Try to get the thread root event
			if let Ok(root_pdu) = services.rooms.timeline.get_pdu(target).await {
				// Check visibility
				if services
					.rooms
					.state_accessor
					.user_can_see_event(sender_user, room_id, target)
					.await
				{
					// Store the root event to add to the response
					root_event = Some(root_pdu);
				}
			}
		}
	}

	// Determine if there are more events to fetch
	let has_more = if root_event.is_some() {
		false // We've included the root, no more events
	} else {
		// Check if we got a full page of results (might be more)
		events.len() >= limit
	};

	let next_batch = if has_more {
		match dir {
			| Direction::Forward => events.last(),
			| Direction::Backward => events.first(),
		}
		.map(|(count, _)| count.to_string())
	} else {
		None
	};

	// Build the response chunk with thread root if needed
	let chunk: Vec<_> = root_event
		.into_iter()
		.map(Event::into_format)
		.chain(events.into_iter().map(at!(1)).map(Event::into_format))
		.collect();

	Ok(get_relating_events::v1::Response {
		next_batch,
		prev_batch: from.map(Into::into),
		recursion_depth: recurse.then_some(depth.into()),
		chunk,
	})
}

async fn visibility_filter<Pdu: Event + Send + Sync>(
	services: &Services,
	sender_user: &UserId,
	item: (PduCount, Pdu),
) -> Option<(PduCount, Pdu)> {
	let (_, pdu) = &item;

	services
		.rooms
		.state_accessor
		.user_can_see_event(sender_user, &pdu.room_id_or_hash(), pdu.event_id())
		.await
		.then_some(item)
}
