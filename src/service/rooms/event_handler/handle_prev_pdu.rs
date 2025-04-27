use std::{collections::BTreeMap, time::Instant};

use conduwuit::{
	Err, Event, PduEvent, Result, debug::INFO_SPAN_LEVEL, defer, implement,
	utils::continue_exponential_backoff_secs,
};
use ruma::{CanonicalJsonValue, EventId, MilliSecondsSinceUnixEpoch, RoomId, ServerName};
use tracing::debug;

#[implement(super::Service)]
#[allow(clippy::type_complexity)]
#[allow(clippy::too_many_arguments)]
#[tracing::instrument(
	name = "prev",
	level = INFO_SPAN_LEVEL,
	skip_all,
	fields(%prev_id),
)]
pub(super) async fn handle_prev_pdu<'a, Pdu>(
	&self,
	origin: &'a ServerName,
	event_id: &'a EventId,
	room_id: &'a RoomId,
	eventid_info: Option<(PduEvent, BTreeMap<String, CanonicalJsonValue>)>,
	create_event: &'a Pdu,
	first_ts_in_room: MilliSecondsSinceUnixEpoch,
	prev_id: &'a EventId,
) -> Result
where
	Pdu: Event + Send + Sync,
{
	// Check for disabled again because it might have changed
	if self.services.metadata.is_disabled(room_id).await {
		return Err!(Request(Forbidden(debug_warn!(
			"Federaton of room {room_id} is currently disabled on this server. Request by \
			 origin {origin} and event ID {event_id}"
		))));
	}

	if let Some((time, tries)) = self
		.services
		.globals
		.bad_event_ratelimiter
		.read()
		.expect("locked")
		.get(prev_id)
	{
		// Exponential backoff
		const MIN_DURATION: u64 = 5 * 60;
		const MAX_DURATION: u64 = 60 * 60 * 24;
		if continue_exponential_backoff_secs(MIN_DURATION, MAX_DURATION, time.elapsed(), *tries) {
			debug!(
				?tries,
				duration = ?time.elapsed(),
				"Backing off from prev_event"
			);
			return Ok(());
		}
	}

	let Some((pdu, json)) = eventid_info else {
		return Ok(());
	};

	// Skip old events
	if pdu.origin_server_ts() < first_ts_in_room {
		return Ok(());
	}

	let start_time = Instant::now();
	self.federation_handletime
		.write()
		.expect("locked")
		.insert(room_id.into(), ((*prev_id).to_owned(), start_time));

	defer! {{
		self.federation_handletime
			.write()
			.expect("locked")
			.remove(room_id);
	}};

	self.upgrade_outlier_to_timeline_pdu(pdu, json, create_event, origin, room_id)
		.await?;

	debug!(
		elapsed = ?start_time.elapsed(),
		"Handled prev_event",
	);

	Ok(())
}
