use axum::extract::State;
use conduwuit::{
	Err, Event, Result, at,
	utils::{BoolExt, stream::TryTools},
};
use futures::{FutureExt, TryStreamExt, future::try_join4};
use ruma::api::client::room::initial_sync::v3::{PaginationChunk, Request, Response};

use crate::Ruma;

const LIMIT_MAX: usize = 100;

pub(crate) async fn room_initial_sync_route(
	State(services): State<crate::State>,
	body: Ruma<Request>,
) -> Result<Response> {
	let room_id = &body.room_id;

	if !services
		.rooms
		.state_accessor
		.user_can_see_state_events(body.sender_user(), room_id)
		.await
	{
		return Err!(Request(Forbidden("No room preview available.")));
	}

	let membership = services
		.rooms
		.state_cache
		.user_membership(body.sender_user(), room_id)
		.map(Ok);

	let visibility = services.rooms.directory.visibility(room_id).map(Ok);

	let state = services
		.rooms
		.state_accessor
		.room_state_full_pdus(room_id)
		.map_ok(Event::into_format)
		.try_collect::<Vec<_>>();

	let limit = LIMIT_MAX;
	let events = services
		.rooms
		.timeline
		.pdus_rev(None, room_id, None)
		.try_take(limit)
		.try_collect::<Vec<_>>();

	let (membership, visibility, state, events) =
		try_join4(membership, visibility, state, events)
			.boxed()
			.await?;

	let messages = PaginationChunk {
		start: events.last().map(at!(0)).as_ref().map(ToString::to_string),

		end: events
			.first()
			.map(at!(0))
			.as_ref()
			.map(ToString::to_string)
			.unwrap_or_default(),

		chunk: events
			.into_iter()
			.map(at!(1))
			.map(Event::into_format)
			.collect(),
	};

	Ok(Response {
		room_id: room_id.to_owned(),
		account_data: None,
		state: state.into(),
		messages: messages.chunk.is_empty().or_some(messages),
		visibility: visibility.into(),
		membership,
	})
}
