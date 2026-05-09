use std::{
	cmp::min,
	collections::{BTreeMap, HashMap, HashSet, VecDeque, hash_map},
	time::Instant,
};

use conduwuit::{
	Err, Event, PduEvent, debug, debug_info, debug_warn, err,
	matrix::event::gen_event_id_canonical_json, trace, utils::continue_exponential_backoff_secs,
	warn,
};
use futures::StreamExt;
use ruma::{
	CanonicalJsonValue, EventId, OwnedEventId, OwnedRoomId, OwnedServerName, RoomId, ServerName,
	api::federation::event::{get_event, get_missing_events},
};

use super::get_room_version_rules;

impl super::Service {
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
	pub(super) async fn backfill_missing_events(
		&self,
		room_id: OwnedRoomId,
		latest_events: Vec<OwnedEventId>,
		via: OwnedServerName,
	) -> conduwuit::Result<HashMap<OwnedEventId, PduEvent>> {
		if latest_events.is_empty() {
			return Ok(HashMap::new());
		}
		let earliest_events = self
			.services
			.state
			.get_forward_extremities(&room_id)
			.collect::<Vec<_>>()
			.await;
		// TODO: min_depth is probably necessary to avoid fetching the entire room
		// history if there are very long gaps
		let mut latest_events = latest_events;
		let mut loop_count: u64 = 3;
		// Start with a base number of 3 so that we fetch 10, 16, 25, 36, etc
		// instead of 1, 2, 4, 9, so on.
		let mut backfilled_events = HashMap::with_capacity(10);

		while !latest_events.is_empty() {
			let mut request = get_missing_events::v1::Request::new(
				room_id.clone(),
				earliest_events.clone(),
				latest_events.clone(),
			);
			request.limit = min(loop_count.saturating_pow(2), 100)
				.try_into()
				.expect("limit cannot be greater than 100, which fits into UInt");
			if backfilled_events.len() > 1000 {
				warn!(
					"Received {} missing events, refusing to fetch more (infinite loop?)",
					backfilled_events.len()
				);
				break;
			}

			debug_info!(
				backfilled=%backfilled_events.len(),
				%loop_count,
				"Asking {via} for up to {} missing events",
				request.limit
			);
			trace!(
				?latest_events,
				?earliest_events,
				%via,
				limit=%request.limit,
				"Requesting missing events"
			);
			let response: get_missing_events::v1::Response = self
				.services
				.sending
				.send_federation_request(&via, request)
				.await?;
			loop_count = loop_count.saturating_add(1);

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
				let (incoming_room_id, event_id, pdu_json) =
					self.parse_incoming_pdu(&event).await.map_err(|e| {
						err!(BadServerResponse("{via} returned an invalid event: {e:?}"))
					})?;
				if incoming_room_id != room_id {
					return Err!(BadServerResponse(
						"{via} returned {event_id} in missing events which belongs to \
						 {incoming_room_id}, not {room_id}"
					));
				}
				if backfilled_events.contains_key(&event_id) {
					debug_warn!(%via, %event_id, "Remote retransmitted event");
					continue;
				}
				// TODO: Should this be scoped to the GME session? We might end up incorrectly
				// assuming we've caught up if we do this
				if self.services.timeline.pdu_exists(&event_id).await {
					debug!(%via, %event_id, "Already seen event in database");
					continue;
				}
				unseen = unseen.saturating_add(1);
				let parsed = PduEvent::from_id_val(&event_id, pdu_json)
					.map_err(|e| err!(BadServerResponse("Unable to parse {event_id}: {e}")))?;
				backfilled_events.insert(event_id, parsed.clone());
				for prev_event_id in parsed.prev_events() {
					if backfilled_events.contains_key(prev_event_id)
						|| self.services.timeline.pdu_exists(prev_event_id).await
					{
						continue;
					}
					latest_events.push(prev_event_id.to_owned());
				}
			}
			latest_events.retain(|e| !backfilled_events.contains_key(e));
			debug!(
				chunk=%chunk_len,
				remaining=%latest_events.len(),
				"Got missing events"
			);
		}

		Ok(backfilled_events)
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
