use axum::extract::State;
use conduwuit::{
	Result, at, debug_warn,
	matrix::{
		Event,
		pdu::{PduCount, PduEvent},
	},
};
use futures::StreamExt;
use ruma::{api::client::threads::get_threads, uint};

use crate::Ruma;

/// # `GET /_matrix/client/r0/rooms/{roomId}/threads`
pub(crate) async fn get_threads_route(
	State(services): State<crate::State>,
	ref body: Ruma<get_threads::v1::Request>,
) -> Result<get_threads::v1::Response> {
	// Use limit or else 10, with maximum 100
	let limit = body
		.limit
		.unwrap_or_else(|| uint!(10))
		.try_into()
		.unwrap_or(10)
		.min(100);

	let from: PduCount = body
		.from
		.as_deref()
		.map(str::parse)
		.transpose()?
		.unwrap_or_else(PduCount::max);

	let threads: Vec<(PduCount, PduEvent)> = services
		.rooms
		.threads
		.threads_until(body.sender_user(), &body.room_id, from, &body.include)
		.await?
		.take(limit)
		.filter_map(|(count, pdu)| async move {
			services
				.rooms
				.state_accessor
				.user_can_see_event(body.sender_user(), &body.room_id, &pdu.event_id)
				.await
				.then_some((count, pdu))
		})
		.then(|(count, mut pdu)| async move {
			if let Err(e) = services
				.rooms
				.pdu_metadata
				.add_bundled_aggregations_to_pdu(body.sender_user(), &mut pdu)
				.await
			{
				debug_warn!("Failed to add bundled aggregations to thread: {e}");
			}
			(count, pdu)
		})
		.collect()
		.await;

	Ok(get_threads::v1::Response {
		next_batch: threads
			.last()
			.filter(|_| threads.len() >= limit)
			.map(at!(0))
			.as_ref()
			.map(ToString::to_string),

		chunk: threads
			.into_iter()
			.map(at!(1))
			.map(Event::into_format)
			.collect(),
	})
}
