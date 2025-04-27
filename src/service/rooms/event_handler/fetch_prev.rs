use std::{
	collections::{BTreeMap, HashMap, HashSet, VecDeque},
	iter::once,
};

use conduwuit::{
	Event, PduEvent, Result, debug_warn, err, implement,
	state_res::{self},
};
use futures::{FutureExt, future};
use ruma::{
	CanonicalJsonValue, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId, RoomId, ServerName,
	int, uint,
};

use super::check_room_id;

#[implement(super::Service)]
#[tracing::instrument(
    level = "debug",
	skip_all,
	fields(%origin),
)]
#[allow(clippy::type_complexity)]
pub(super) async fn fetch_prev<'a, Pdu, Events>(
	&self,
	origin: &ServerName,
	create_event: &Pdu,
	room_id: &RoomId,
	first_ts_in_room: MilliSecondsSinceUnixEpoch,
	initial_set: Events,
) -> Result<(
	Vec<OwnedEventId>,
	HashMap<OwnedEventId, (PduEvent, BTreeMap<String, CanonicalJsonValue>)>,
)>
where
	Pdu: Event + Send + Sync,
	Events: Iterator<Item = &'a EventId> + Clone + Send,
{
	let num_ids = initial_set.clone().count();
	let mut eventid_info = HashMap::new();
	let mut graph: HashMap<OwnedEventId, _> = HashMap::with_capacity(num_ids);
	let mut todo_outlier_stack: VecDeque<OwnedEventId> =
		initial_set.map(ToOwned::to_owned).collect();

	let mut amount = 0;

	while let Some(prev_event_id) = todo_outlier_stack.pop_front() {
		self.services.server.check_running()?;

		match self
			.fetch_and_handle_outliers(
				origin,
				once(prev_event_id.as_ref()),
				create_event,
				room_id,
			)
			.boxed()
			.await
			.pop()
		{
			| Some((pdu, mut json_opt)) => {
				check_room_id(room_id, &pdu)?;

				let limit = self.services.server.config.max_fetch_prev_events;
				if amount > limit {
					debug_warn!("Max prev event limit reached! Limit: {limit}");
					graph.insert(prev_event_id.clone(), HashSet::new());
					continue;
				}

				if json_opt.is_none() {
					json_opt = self
						.services
						.outlier
						.get_outlier_pdu_json(&prev_event_id)
						.await
						.ok();
				}

				if let Some(json) = json_opt {
					if pdu.origin_server_ts() > first_ts_in_room {
						amount = amount.saturating_add(1);
						for prev_prev in pdu.prev_events() {
							if !graph.contains_key(prev_prev) {
								todo_outlier_stack.push_back(prev_prev.to_owned());
							}
						}

						graph.insert(
							prev_event_id.clone(),
							pdu.prev_events().map(ToOwned::to_owned).collect(),
						);
					} else {
						// Time based check failed
						graph.insert(prev_event_id.clone(), HashSet::new());
					}

					eventid_info.insert(prev_event_id.clone(), (pdu, json));
				} else {
					// Get json failed, so this was not fetched over federation
					graph.insert(prev_event_id.clone(), HashSet::new());
				}
			},
			| _ => {
				// Fetch and handle failed
				graph.insert(prev_event_id.clone(), HashSet::new());
			},
		}
	}

	let event_fetch = |event_id| {
		let origin_server_ts = eventid_info
			.get(&event_id)
			.map_or_else(|| uint!(0), |info| info.0.origin_server_ts().get());

		// This return value is the key used for sorting events,
		// events are then sorted by power level, time,
		// and lexically by event_id.
		future::ok((int!(0), MilliSecondsSinceUnixEpoch(origin_server_ts)))
	};

	let sorted = state_res::lexicographical_topological_sort(&graph, &event_fetch)
		.await
		.map_err(|e| err!(Database(error!("Error sorting prev events: {e}"))))?;

	Ok((sorted, eventid_info))
}
