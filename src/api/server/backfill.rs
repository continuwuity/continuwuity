use std::cmp;

use axum::extract::State;
use conduwuit::{
	Err, Event, PduCount, Result, info,
	result::LogErr,
	utils::{IterStream, ReadyExt, stream::TryTools},
};
use futures::{FutureExt, StreamExt, TryStreamExt};
use ruma::{MilliSecondsSinceUnixEpoch, api::federation::backfill::get_backfill};

use super::AccessCheck;
use crate::Ruma;

/// arbitrary number but synapse's is 100 and we can handle lots of these
/// anyways
const LIMIT_MAX: usize = 150;
/// no spec defined number but we can handle a lot of these
const LIMIT_DEFAULT: usize = 50;

/// # `GET /_matrix/federation/v1/backfill/<room_id>`
///
/// Retrieves events from before the sender joined the room, if the room's
/// history visibility allows.
pub(crate) async fn get_backfill_route(
	State(services): State<crate::State>,
	ref body: Ruma<get_backfill::v1::Request>,
) -> Result<get_backfill::v1::Response> {
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
			"Refusing to serve backfill for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	let limit = body
		.limit
		.try_into()
		.unwrap_or(LIMIT_DEFAULT)
		.min(LIMIT_MAX);

	let from = body
		.v
		.iter()
		.stream()
		.filter_map(|event_id| {
			services
				.rooms
				.timeline
				.get_pdu_count(event_id)
				.map(Result::ok)
		})
		.ready_fold(PduCount::min(), cmp::max)
		.await;

	Ok(get_backfill::v1::Response {
		origin_server_ts: MilliSecondsSinceUnixEpoch::now(),

		origin: services.globals.server_name().to_owned(),

		pdus: services
			.rooms
			.timeline
			.pdus_rev(&body.room_id, Some(from.saturating_add(1)))
			.try_take(limit)
			.try_filter_map(|(_, pdu)| async move {
				Ok(services
					.rooms
					.state_accessor
					.server_can_see_event(body.origin(), &pdu.room_id_or_hash(), &pdu.event_id)
					.await
					.then_some(pdu))
			})
			.and_then(async |mut pdu| {
				// Strip the transaction ID, as that is private
				pdu.remove_transaction_id().log_err().ok();
				// Add age, as this is specified
				pdu.add_age().log_err().ok();
				// It's not clear if we should strip or add any more data, leave as is.
				// In particular: Redaction?
				Ok(pdu)
			})
			.try_filter_map(|pdu| async move {
				Ok(services
					.rooms
					.timeline
					.get_pdu_json(&pdu.event_id)
					.await
					.ok())
			})
			.and_then(|pdu| {
				services
					.sending
					.convert_to_outgoing_federation_event(pdu)
					.map(Ok)
			})
			.try_collect()
			.await?,
	})
}
