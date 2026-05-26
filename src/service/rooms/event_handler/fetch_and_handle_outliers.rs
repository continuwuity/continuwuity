use std::{
	collections::{BTreeMap, HashMap, HashSet, VecDeque, hash_map},
	time::Instant,
};

use assign::assign;
use conduwuit::{
	Event, PduEvent, debug, debug_info, debug_warn, err, error,
	matrix::event::gen_event_id_canonical_json,
	state_res::lexicographical_topological_sort,
	trace,
	utils::{IterStream, continue_exponential_backoff_secs, stream::BroadbandExt},
	warn,
};
use futures::StreamExt;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId,
	RoomId, ServerName, UInt,
	api::federation::event::{get_event, get_missing_events},
	int,
};

use super::get_room_version_rules;

/// Attempts to build a localised directed acyclic graph out of the given PDUs,
/// returning them in a topologically sorted order.
///
/// This is used to attempt to process PDUs in an order that respects their
/// dependencies, however it is ultimately the sender's responsibility to send
/// them in a processable order, so this is just a best effort attempt. It does
/// not account for power levels or other tie breaks.
pub async fn build_local_dag<S: std::hash::BuildHasher>(
	pdu_map: &HashMap<OwnedEventId, CanonicalJsonObject, S>,
) -> conduwuit::Result<Vec<OwnedEventId>> {
	debug_assert!(pdu_map.len() >= 2, "needless call to build_local_dag with less than 2 PDUs");
	let mut dag: HashMap<OwnedEventId, HashSet<OwnedEventId>> =
		HashMap::with_capacity(pdu_map.len());
	let mut id_origin_ts: HashMap<OwnedEventId, _> = HashMap::with_capacity(pdu_map.len());

	for (event_id, value) in pdu_map {
		// We already checked that these properties are correct in parse_incoming_pdu,
		// so it's safe to unwrap here.
		// We also filter to remove any prev_events that are not in this pdu_map, as we
		// need to have at least one event with zero out degrees for the lexico-topo
		// sort below. If there are multiple events with omitted prevs, they will be
		// ordered by timestamp, then event ID. At that point though, it's unlikely to
		// matter.
		let prev_events = value
			.get("prev_events")
			.unwrap()
			.as_array()
			.unwrap()
			.iter()
			.map(|v| EventId::parse(v.as_str().unwrap()).unwrap())
			.filter(|id| pdu_map.contains_key(id))
			.collect();

		dag.insert(event_id.clone(), prev_events);
		let origin_server_ts = value
			.get("origin_server_ts")
			.and_then(CanonicalJsonValue::as_integer)
			.unwrap_or_default();
		id_origin_ts.insert(event_id.clone(), origin_server_ts);
	}

	debug!(count = dag.len(), "Sorting incoming events with partial graph");
	lexicographical_topological_sort(&dag, &async |node_id| {
		// Note: we don't bother fetching power levels because that would massively slow
		// this function down. This is a best-effort attempt to order events correctly
		// for processing, however ultimately that should be the sender's job.
		let ts = id_origin_ts
			.get(&node_id)
			.copied()
			.unwrap_or_else(|| int!(0))
			.to_string()
			.parse::<u64>()
			.ok()
			.and_then(UInt::new)
			.unwrap_or_default();
		Ok((int!(0), MilliSecondsSinceUnixEpoch(ts)))
	})
	.await
	.inspect(|sorted| {
		debug_assert_eq!(
			sorted.len(),
			pdu_map.len(),
			"Sorted graph was not the same size as the input graph"
		);
	})
	.map_err(|e| err!("failed to resolve local graph: {e}"))
}

impl super::Service {
	/// Uses `/_matrix/federation/v1/get_missing_events` to fill gaps in the
	/// DAG.
	///
	/// When this function is called, the "earliest events" (current forward
	/// extremities) will be collected, and the function will loop with an
	/// exponentially incrementing limit (up to 100 per request) until it has
	/// filled the gap, i.e. when the remote says there's no more events.
	///
	/// This function will iterate until the remote returns no more events,
	/// increasing the limit by a factor of 10. If 100 iterations are reached or
	/// max_fetch_prev_events events are backfilled, the function will give up
	/// and return what it has, to avoid pulling in too much data (for example,
	/// absurdly large gaps).
	///
	/// This function does not persist the events. The caller is responsible for
	/// passing them through handle_incoming_pdu.
	///
	/// ## Parameters
	///
	/// - `room_id`: The room's ID.
	/// - `head`: The event we are potentially missing prev_events for.
	/// - `tail`: The most recently known events in the graph (typically forward
	///   extremities).
	/// - `via`: The server to ask for missing events.
	/// - `min_depth`: Don't process events with a `depth` lower than this
	///   value. Not massively useful, but can help short-circuit infinite loops
	///   and weird edge paths.
	pub async fn get_missing_events(
		&self,
		room_id: &RoomId,
		head: &PduEvent,
		tail: Vec<OwnedEventId>,
		via: &ServerName,
		min_depth: UInt,
	) -> conduwuit::Result<HashMap<OwnedEventId, PduEvent>> {
		#[cfg(debug_assertions)]
		{
			let missing_count = head
				.prev_events()
				.stream()
				.broad_filter_map(|event_id| async move {
					match self
						.services
						.timeline
						.get_non_outlier_pdu_json(event_id)
						.await
						.inspect(|_| debug!("Found prev_event {event_id} locally."))
						.inspect_err(
							|e| debug!(%e, "Could not find prev_event {event_id} locally."),
						) {
						| Ok(_) => None,
						| Err(_) => Some(event_id),
					}
				})
				.count()
				.await;
			debug_assert_ne!(
				missing_count, 0,
				"event passed to get_missing_events is not missing any events (wasteful call)"
			);
		};

		let mut discovered = HashMap::with_capacity(20);
		let mut latest_events = vec![head.event_id().to_owned()];
		let mut iterations = 0_u8;
		loop {
			iterations = iterations.saturating_add(1);
			let limit = iterations.saturating_mul(10).min(100);
			debug_info!(%limit, %via, %iterations, discovered=discovered.len(), %min_depth, "Attempting to gap fill missing events");
			let response: get_missing_events::v1::Response = self
				.services
				.sending
				.send_federation_request(
					via,
					assign!(
						get_missing_events::v1::Request::new(
							room_id.to_owned(),
							tail.clone(),
							latest_events.clone()
						),
						{limit: limit.into(), min_depth}
					),
				)
				.await?;

			if response.events.is_empty() {
				debug_info!(%via, "Finished gap filling missing events (remote returned no more events).");
				break;
			}
			debug_info!("Got {} events back from remote", response.events.len());

			latest_events.clear();
			for raw_event in response.events {
				let (_, event_id, pdu_json) = self.parse_incoming_pdu(&raw_event).await?;
				let pdu = PduEvent::from_id_val(&event_id, pdu_json).map_err(|e| {
					err!(Request(BadJson("Failed to parse backfilled event {event_id}: {e}")))
				})?;

				if pdu.depth < min_depth {
					debug_warn!(
						"Received PDU with depth {} below min_depth {}, ignoring",
						pdu.depth,
						min_depth
					);
					continue;
				}

				for prev_event_id in pdu.prev_events() {
					if discovered.contains_key(prev_event_id) {
						continue;
					}
					if self
						.services
						.timeline
						.non_outlier_pdu_exists(prev_event_id)
						.await
					{
						continue;
					}
					latest_events.push(prev_event_id.to_owned());
					break;
				}

				discovered.insert(event_id.clone(), pdu);
			}

			if latest_events.is_empty() {
				break;
			} else if discovered.len() > self.services.server.config.max_fetch_prev_events.into()
				|| iterations >= 20
			{
				error!(
					filled=discovered.len(),
					max_fetch_prev_events=self.services.server.config.max_fetch_prev_events,
					%iterations,
					"Gap too large, giving up"
				);
				break;
			}
		}

		Ok(discovered)
	}

	/// Find the event and auth it. Once the event is validated (steps 1 - 8)
	/// it is appended to the outliers Tree.
	///
	/// Returns pdu and if we fetched it over federation the raw json.
	///
	/// a. Look in the main timeline (pduid_pdu tree)
	/// b. Look at outlier pdu tree
	/// c. Ask origin server over federation
	/// d. TODO: Ask other servers over federation?
	#[deprecated]
	pub(super) async fn fetch_and_handle_outliers<'a, Pdu, Events>(
		&self,
		origin: &'a ServerName,
		events: Events,
		create_event: &'a Pdu,
		room_id: &'a RoomId,
	) -> Vec<(PduEvent, Option<BTreeMap<String, CanonicalJsonValue>>)>
	where
		Pdu: Event + Send + Sync,
		Events: Iterator<Item = &'a EventId> + Clone + Send,
	{
		let back_off = |id| match self
			.services
			.globals
			.bad_event_ratelimiter
			.write()
			.entry(id)
		{
			| hash_map::Entry::Vacant(e) => {
				e.insert((Instant::now(), 1));
			},
			| hash_map::Entry::Occupied(mut e) => {
				*e.get_mut() = (Instant::now(), e.get().1.saturating_add(1));
			},
		};

		let mut events_with_auth_events = Vec::with_capacity(events.clone().count());
		trace!("Fetching {} outlier pdus", events.clone().count());

		for id in events {
			// a. Look in the main timeline (pduid_pdu tree)
			// b. Look at outlier pdu tree
			// (get_pdu_json checks both)
			if let Ok(local_pdu) = self.services.timeline.get_pdu(id).await {
				trace!("Found {id} in main timeline or outlier tree");
				events_with_auth_events.push((id.to_owned(), Some(local_pdu), vec![]));
				continue;
			}

			// c. Ask origin server over federation
			// We also handle its auth chain here so we don't get a stack overflow in
			// handle_outlier_pdu.
			let mut todo_auth_events: VecDeque<_> = [id.to_owned()].into();
			let mut events_in_reverse_order = Vec::with_capacity(todo_auth_events.len());

			let mut events_all = HashSet::with_capacity(todo_auth_events.len());
			while let Some(next_id) = todo_auth_events.pop_front() {
				if let Some((time, tries)) = self
					.services
					.globals
					.bad_event_ratelimiter
					.read()
					.get(&*next_id)
				{
					// Exponential backoff
					const MIN_DURATION: u64 = 60 * 2;
					const MAX_DURATION: u64 = 60 * 60 * 8;
					if continue_exponential_backoff_secs(
						MIN_DURATION,
						MAX_DURATION,
						time.elapsed(),
						*tries,
					) {
						debug_warn!(
							tried = ?*tries,
							elapsed = ?time.elapsed(),
							"Backing off from {next_id}",
						);
						continue;
					}
				}

				if events_all.contains(&next_id) {
					continue;
				}

				if self.services.timeline.pdu_exists(&next_id).await {
					trace!("Found {next_id} in db");
					continue;
				}

				debug!("Fetching {next_id} over federation from {origin}.");
				match self
					.services
					.sending
					.send_federation_request(
						origin,
						get_event::v1::Request::new((*next_id).to_owned()),
					)
					.await
				{
					| Ok(res) => {
						debug!("Got {next_id} over federation from {origin}");
						let Ok(room_version_rules) = get_room_version_rules(create_event) else {
							back_off((*next_id).to_owned());
							continue;
						};

						let Ok((calculated_event_id, value)) =
							gen_event_id_canonical_json(&res.pdu, &room_version_rules)
						else {
							back_off((*next_id).to_owned());
							continue;
						};

						if calculated_event_id != *next_id {
							warn!(
								"Server didn't return event id we requested: requested: \
								 {next_id}, we got {calculated_event_id}. Event: {:?}",
								&res.pdu
							);
						}

						if let Some(auth_events) = value
							.get("auth_events")
							.and_then(CanonicalJsonValue::as_array)
						{
							for auth_event in auth_events {
								match serde_json::from_value::<OwnedEventId>(
									auth_event.clone().into(),
								) {
									| Ok(auth_event) => {
										trace!(
											"Found auth event id {auth_event} for event \
											 {next_id}"
										);
										todo_auth_events.push_back(auth_event);
									},
									| _ => {
										warn!("Auth event id is not valid");
									},
								}
							}
						} else {
							warn!("Auth event list invalid");
						}

						events_in_reverse_order.push((next_id.clone(), value));
						events_all.insert(next_id);
					},
					| Err(e) => {
						warn!("Failed to fetch auth event {next_id} from {origin}: {e}");
						back_off((*next_id).to_owned());
					},
				}
			}

			events_with_auth_events.push((id.to_owned(), None, events_in_reverse_order));
		}

		let mut pdus = Vec::with_capacity(events_with_auth_events.len());
		for (id, local_pdu, events_in_reverse_order) in events_with_auth_events {
			// a. Look in the main timeline (pduid_pdu tree)
			// b. Look at outlier pdu tree
			// (get_pdu_json checks both)
			if let Some(local_pdu) = local_pdu {
				trace!("Found {id} in main timeline or outlier tree");
				pdus.push((local_pdu.clone(), None));
			}

			for (next_id, value) in events_in_reverse_order.into_iter().rev() {
				if let Some((time, tries)) = self
					.services
					.globals
					.bad_event_ratelimiter
					.read()
					.get(&*next_id)
				{
					// Exponential backoff
					const MIN_DURATION: u64 = 5 * 60;
					const MAX_DURATION: u64 = 60 * 60 * 24;
					if continue_exponential_backoff_secs(
						MIN_DURATION,
						MAX_DURATION,
						time.elapsed(),
						*tries,
					) {
						debug!("Backing off from {next_id}");
						continue;
					}
				}

				trace!("Handling outlier {next_id}");
				match Box::pin(self.handle_outlier_pdu(
					origin,
					create_event,
					&next_id,
					room_id,
					value.clone(),
					true,
				))
				.await
				{
					| Ok((pdu, json)) =>
						if next_id == *id {
							trace!("Handled outlier {next_id} (original request)");
							pdus.push((pdu, Some(json)));
						},
					| Err(e) => {
						warn!("Authentication of event {next_id} failed: {e:?}");
						back_off(next_id);
					},
				}
			}
		}
		trace!("Fetched and handled {} outlier pdus", pdus.len());
		pdus
	}
}
