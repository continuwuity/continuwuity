use std::collections::{HashMap, HashSet, VecDeque};

use assign::assign;
use conduwuit::{
	Err, Event, PduEvent, debug, debug_error, debug_info, debug_warn, err, error,
	state_res::lexicographical_topological_sort,
	trace,
	utils::{IterStream, stream::BroadbandExt},
	warn,
};
use futures::{StreamExt, future::select_ok};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId,
	OwnedRoomId, OwnedServerName, RoomId, ServerName, UInt,
	api::federation::event::{get_event, get_missing_events},
	int,
	room_version_rules::RoomVersionRules,
};

use super::get_room_version_rules;
use crate::rooms::event_handler::parse_incoming_pdu::expect_event_id_array;

const GET_MISSING_EVENTS_MAX_BATCH_SIZE: u16 = 50; // matches src/server/get_missing_events.rs#LIMIT_MAX

/// Attempts to build a localised directed acyclic graph out of the given PDUs,
/// returning them in a topologically sorted order.
///
/// This is used to attempt to process PDUs in an order that respects their
/// dependencies, however it is ultimately the sender's responsibility to send
/// them in a processable order, so this is just a best effort attempt. It does
/// not account for power levels or other tie breaks.
pub async fn build_local_dag<S: std::hash::BuildHasher + Send + Sync>(
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
	async fn fetch_and_handle_missing_event_via(
		&self,
		remote: OwnedServerName,
		event_id: OwnedEventId,
		room_version_rules: &RoomVersionRules,
	) -> conduwuit::Result<(OwnedEventId, CanonicalJsonObject)> {
		let res = self
			.services
			.sending
			.send_federation_request(&remote, get_event::v1::Request::new(event_id.clone()))
			.await?;

		let (calculated_event_id, value) = self
			.parse_incoming_pdu_with_known_room(&res.pdu, room_version_rules)
			.await?;

		if calculated_event_id != event_id {
			Err!(Request(BadJson(warn!(
				expected=%event_id,
				received=%calculated_event_id,
				"Server didn't return event id we requested",
			))))
		} else {
			Ok((event_id, value))
		}
	}

	/// Asks remote servers for any individual events that are missing. Should
	/// only be used for fetching missing auth events or resolving missing
	/// events from state_ids. For all other uses, use get_missing_events.
	pub(super) async fn fetch_and_handle_missing_events<'a, Pdu>(
		&self,
		origin: &'a ServerName,
		events: Vec<OwnedEventId>,
		create_event: &'a Pdu,
		room_id: &'a RoomId,
	) -> HashMap<OwnedEventId, PduEvent>
	where
		Pdu: Event + Send + Sync,
	{
		let room_version_rules =
			&get_room_version_rules(create_event).unwrap_or(RoomVersionRules::V1);
		let mut candidates = self
			.services
			.timeline
			.candidate_backfill_servers(room_id)
			.await;
		candidates.insert(origin.to_owned());
		candidates.retain(|sn| self.services.globals.server_name() != sn);
		assert_ne!(candidates.len(), 0, "no candidates to fetch missing events from");
		let mut seeded_events =
			HashMap::with_capacity(events.len().saturating_add(events.len().saturating_mul(3)));
		trace!(
			"Fetching {} unknown PDUs on demand from {} candidates",
			events.len(),
			candidates.len()
		);

		let mut seen: HashMap<OwnedEventId, u8> = HashMap::new();
		for id in events {
			let mut todo: VecDeque<OwnedEventId> = [id.clone()].into();
			while let Some(next_id) = todo.pop_front() {
				if seeded_events.contains_key(&next_id) {
					continue;
				}
				if let Ok(local_pdu) = self.services.timeline.get_pdu(&next_id).await {
					trace!("Found {next_id} in db");
					seeded_events.insert(next_id.clone(), local_pdu.into_canonical_object());
					continue;
				}
				let attempts = seen.get(&*next_id).copied().unwrap_or_default();
				if attempts >= 5 {
					debug_error!(%attempts, %next_id, "Could not fetch missing event after 5 attempts, giving up");
					continue;
				}

				debug!("Fetching {next_id} over federation");
				let futures = candidates
					.iter()
					.map(|remote| {
						Box::pin(self.fetch_and_handle_missing_event_via(
							remote.clone(),
							next_id.clone(),
							room_version_rules,
						))
					})
					.collect::<Vec<_>>();
				let (event_id, value) = match select_ok(futures).await {
					| Ok((x, _)) => x,
					| Err(e) => {
						warn!("failed to fetch missing event {next_id} from any candidate: {e}");
						continue;
					},
				};
				let auth_events =
					match expect_event_id_array(&value, "auth_events").map_err(|e| {
						err!(Request(BadJson(warn!(
							%event_id,
							"Failed to parse event fetched from remote: {e}"
						))))
					}) {
						| Ok(auth_events) => auth_events,
						| Err(e) => {
							warn!(
								?e,
								"event {event_id} is malformed (bad auth_events), skipping"
							);
							continue;
						},
					};
				let mut have_all_auth = true;
				for auth_event_id in auth_events {
					if let Ok(local_pdu) = self.services.timeline.get_pdu(&next_id).await {
						trace!("Found auth event {next_id} in db");
						seeded_events.insert(id.clone(), local_pdu.into_canonical_object());
						continue;
					}
					if seeded_events.contains_key(&auth_event_id) {
						trace!(%auth_event_id, "Already found auth event");
						continue;
					}
					debug!("Missing auth event {auth_event_id} for event {next_id}");
					seen.insert(auth_event_id.clone(), attempts.saturating_add(1));
					todo.push_back(auth_event_id);
					have_all_auth = false;
				}
				// Insert this PDU back at the end of the queue so that it will be resolved once
				// all of its auth events have been fetched.
				// TODO: This may result in infinite looping, needs a breaker
				if have_all_auth {
					debug!(%next_id, "Have all auth events");
					seeded_events.insert(next_id, value);
				} else {
					debug_warn!(
						"Fetched {next_id} but missing some auth events, will have to re-fetch."
					);
					seen.insert(next_id.clone(), attempts.saturating_add(1));
					todo.push_back(next_id);
				}
			}
		}

		let seeded_ordered = build_local_dag(
			&seeded_events
				.iter()
				.map(|(eid, e)| (eid.to_owned(), e.clone()))
				.collect::<HashMap<OwnedEventId, CanonicalJsonObject>>(),
		)
		.await
		.expect("failed to build local DAG");
		let mut pdus = HashMap::with_capacity(seeded_ordered.len());
		for id in seeded_ordered {
			let pdu_json = seeded_events.remove(&id).unwrap();
			debug_info!("Handling missing event {id} as outlier");
			match Box::pin(self.handle_outlier_pdu(
				origin,
				create_event,
				&id,
				room_id,
				pdu_json,
				true,
			))
			.await
			{
				| Ok((pdu, _)) => {
					let _ = pdus.insert(id, pdu);
				},
				| Err(e) => warn!("Authentication of event {id} failed: {e:?}"),
			}

			// TODO: should this try to promote to timeline?
			// If we got here, we probably weren't able to promote it before
			// now.
		}

		trace!("Fetched and handled {} missing PDUs", pdus.len());
		pdus
	}

	/// Uses `/_matrix/federation/v1/get_missing_events` to fill gaps in the
	/// DAG.
	///
	/// When this function is called, the "earliest events" (current forward
	/// extremities) will be collected, and the function will loop with an
	/// exponentially incrementing limit (up to 100 per request) until it has
	/// filled the gap, i.e. when the remote says there's no more events.
	///
	/// This function does not persist the events. The caller is responsible for
	/// passing them through handle_incoming_pdu.
	pub async fn backfill_missing_events(
		&self,
		room_id: OwnedRoomId,
		head: HashSet<OwnedEventId>,
		tail: Vec<OwnedEventId>,
		via: OwnedServerName,
	) -> conduwuit::Result<HashMap<OwnedEventId, PduEvent>> {
		if head.is_empty() {
			return Ok(HashMap::new());
		}
		// TODO: min_depth is probably necessary to avoid fetching the entire room
		// history if there are very long gaps
		let mut latest_events = head.clone();
		let mut loop_count: u64 = 3;
		// Start with 3 so that we fetch 9, 16, 25, 36, 49, 64, 81, 100 events.
		// This gives steady growth to the server's typical limit of 100. It's unlikely
		// we'll end up close to that.
		let mut backfilled_events = HashMap::with_capacity(10);

		while !latest_events.is_empty() {
			// TODO: holy clone()
			let frontier_before = latest_events.clone();
			let todo: Vec<OwnedEventId> = latest_events.clone().into_iter().collect();
			let mut request =
				get_missing_events::v1::Request::new(room_id.clone(), tail.clone(), todo.clone());
			let limit = loop_count.saturating_pow(2).min(100);
			request.limit = limit.try_into().expect("limit must fit into UInt");

			debug_info!(
				backfilled=%backfilled_events.len(),
				%loop_count,
				"Asking {via} for up to {limit} missing events",
			);
			trace!(
				?latest_events,
				?tail,
				%via,
				%limit,
				"Requesting missing events"
			);
			let response: get_missing_events::v1::Response = self
				.services
				.sending
				.send_federation_request(&via, request)
				.await?;
			loop_count = loop_count.saturating_add(1);
			trace!(?response, "get_missing_events response");

			// Some buggy servers (including old continuwuity) may return the same events
			// multiple times, which can cause this to be an infinite loop.
			// In order to break this loop, if we see no new events from this response (i.e.
			// all events in the response are already in backfilled_events), we stop,
			// with a warning.
			let mut unseen: usize = 0;
			let chunk_len = response.events.len();
			if response.events.is_empty() {
				debug_info!("No more missing events found");
				break;
			}

			for event in response.events {
				trace!("Parsing incoming event from backfill");
				let (incoming_room_id, event_id, pdu_json) =
					self.parse_incoming_pdu(&event).await.map_err(|e| {
						err!(BadServerResponse("{via} returned an invalid event: {e:?}"))
					})?;
				trace!(%incoming_room_id, %event_id, "Parsed incoming event from backfill");
				if incoming_room_id != room_id {
					return Err!(BadServerResponse(
						"{via} returned {event_id} in missing events which belongs to \
						 {incoming_room_id}, not {room_id}"
					));
				}
				latest_events.remove(&event_id);
				if head.contains(&event_id) || tail.contains(&event_id) {
					debug!("Skipping known event {event_id}");
					continue;
				}
				let retransmitted = backfilled_events.contains_key(&event_id);
				if retransmitted {
					debug_warn!(%via, %event_id, "Remote retransmitted event");
				} else {
					if let Ok(pdu) = self.services.timeline.get_pdu(&event_id).await {
						debug!(%via, %event_id, "Already seen event in database");
						backfilled_events.insert(event_id.clone(), pdu);
					} else {
						unseen = unseen.saturating_add(1);
					}
				}
				let parsed = PduEvent::from_id_val(&event_id, pdu_json)
					.map_err(|e| err!(BadServerResponse("Unable to parse {event_id}: {e}")))?;
				for prev_event_id in parsed.prev_events() {
					if !(backfilled_events.contains_key(prev_event_id)
						|| self.services.timeline.pdu_exists(prev_event_id).await)
					{
						latest_events.insert(prev_event_id.to_owned());
					}
				}
				if !retransmitted {
					backfilled_events.insert(event_id.clone(), parsed);
				}
			}
			latest_events.retain(|event_id| !backfilled_events.contains_key(event_id));
			debug!(
				count=%chunk_len,
				new=%unseen,
				remaining=%latest_events.len(),
				"Got missing events"
			);
			let frontier_changed = latest_events != frontier_before;
			if unseen == 0 && !frontier_changed {
				debug_warn!(
					"Didn't see any new events and the frontier did not change, breaking cycle"
				);
				break;
			}
		}

		debug_info!("Successfully fetched {} missing events from {via}", backfilled_events.len());
		trace!("Missing_events: {backfilled_events:?}");
		Ok(backfilled_events)
	}

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
		assert!(!tail.is_empty(), "empty tail");
		assert_ne!(via, self.services.globals.server_name(), "cannot ask ourselves for events");

		// The iteration limit is in place to ensure that if the remote server leaves us
		// in a state of infinite recursion (as old versions of continuwuity and
		// predecessors would), we give up. However, get_missing_events doesn't return
		// that many events per-request. Synapse returns 20, and conduwuit+ return 50.
		// This means with a hard iteration limit, we might give up too early, before
		// we get a chance to even come close to max_fetch_prev_events. As such, we'll
		// calculate the max limit based on that config option based on these averages.
		let max_fetch = self.services.server.config.max_fetch_prev_events;
		let iteration_limit = max_fetch.saturating_div(20).max(10);

		let mut discovered = HashMap::with_capacity(head.prev_events.len());
		let mut latest_events = vec![head.event_id().to_owned()];
		debug!(
			%room_id,
			event_id=%head.event_id(),
			%iteration_limit,
			"Fetching any missing events for head event",
		);
		for iteration in 0..iteration_limit {
			let limit = iteration
				.saturating_mul(10)
				.min(GET_MISSING_EVENTS_MAX_BATCH_SIZE);
			debug_info!(
				%limit,
				%via,
				%iteration,
				discovered=discovered.len(),
				%min_depth,
				"Attempting to gap fill missing events"
			);
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
					discovered.insert(event_id.clone(), pdu);
					continue;
				}

				for prev_event_id in pdu.prev_events() {
					if discovered.contains_key(prev_event_id) {
						// We already received this event.
						continue;
					}
					if self
						.services
						.timeline
						.non_outlier_pdu_exists(prev_event_id)
						.await
					{
						// NOTE: we explicitly check for *non*-outlier events here, as if we end
						// up discovering outlier events, we will be able to upgrade them
						// immediately.
						continue;
					}
					latest_events.push(prev_event_id.to_owned());
				}

				discovered.insert(event_id.clone(), pdu);
			}

			if latest_events.is_empty() {
				debug!(
					%limit,
					%via,
					%iteration,
					discovered=discovered.len(),
					"No more events to fetch."
				);
				break;
			}
			if discovered.len() >= self.services.server.config.max_fetch_prev_events.into() {
				// Stupid hack, debug_error!() drops the log to a DEBUG when not in debug mode,
				// which is bad because this should at least produce a warning. It's an error in
				// debug mode because this can be important, but typically not much can be done
				// about it as a user.
				#[cfg(debug_assertions)]
				error!(
					discovered=discovered.len(),
					max_fetch_prev_events=self.services.server.config.max_fetch_prev_events,
					%iteration,
					%iteration_limit,
					%via,
					event_id=%head.event_id(),
					%room_id,
					"Encountered a gap too large to fill, giving up"
				);
				#[cfg(not(debug_assertions))]
				warn!(
					discovered=discovered.len(),
					max_fetch_prev_events=self.services.server.config.max_fetch_prev_events,
					%iteration,
					%iteration_limit,
					%via,
					event_id=%head.event_id(),
					%room_id,
					"Encountered a gap too large to fill"
				);
				break;
			}
		}

		Ok(discovered)
	}
}
