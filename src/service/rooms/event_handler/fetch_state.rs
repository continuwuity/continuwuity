use std::{
	cmp::max,
	collections::{HashMap, hash_map},
	time::{Duration, Instant},
};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_warn, err, info, trace,
	utils::{BoolExt, IterStream},
	warn,
};
use futures::{StreamExt, TryFutureExt, future::select_ok};
use ruma::{
	EventId, OwnedEventId, OwnedRoomId, RoomId, ServerName,
	api::federation::event::{get_room_state, get_room_state_ids},
};

use crate::{conduwuit::utils::stream::BroadbandExt, rooms::short::ShortStateKey};

impl super::Service {
	/// Asks a remote server what the state at this event is.
	/// It first attempts to call `GET /_matrix/federation/v1/state_ids` (fast).
	/// If any events are missing, they are fetched from the remote, and
	/// persisted as outliers, before being returned back to this function. If
	/// we are missing a lot of events locally (>=50), this function falls back
	/// to requesting the full state in PDU format from the remote (`GET
	/// /_matrix/federation/v1/state, very slow in large rooms), and persists
	/// them directly.
	#[tracing::instrument(skip_all)]
	pub(super) async fn fetch_state(
		&self,
		origin: &ServerName,
		create_event: &PduEvent,
		room_id: &RoomId,
		event_id: &EventId,
	) -> Result<HashMap<u64, OwnedEventId>> {
		let start = Instant::now();
		trace!(%origin, "Asking remote for state_ids");
		let res: get_room_state_ids::v1::Response = match self
			.services
			.sending
			.send_federation_request(
				origin,
				get_room_state_ids::v1::Request::new(event_id.to_owned(), room_id.to_owned()),
			)
			.await
			.inspect_err(
				|e| debug_warn!(elapsed=?start.elapsed(), "Fetching state for event failed: {e}"),
			) {
			| Ok(resp) => Ok(resp),
			| Err(e) =>
				if e.is_not_found() {
					self.fetch_state_ids_from_backfill_servers(
						event_id.to_owned(),
						room_id.to_owned(),
					)
					.await
				} else {
					Err(e)
				},
		}?;

		debug!(elapsed=?start.elapsed(), events = res.pdu_ids.len(), "Fetching state events");
		let mut state_events: HashMap<OwnedEventId, PduEvent> =
			HashMap::with_capacity(res.pdu_ids.len());
		let to_fetch: Vec<OwnedEventId> = res
			.pdu_ids
			.clone()
			.into_iter()
			.stream()
			.broad_filter_map(|event_id| async move {
				self.services
					.timeline
					.pdu_exists(&event_id)
					.await
					.or_some(event_id)
			})
			.collect()
			.await;
		if to_fetch.is_empty() {
			debug!(elapsed=?start.elapsed(), "All required state events are already known.");
			state_events = res
				.pdu_ids
				.iter()
				.stream()
				.broad_filter_map(|event_id| async move {
					Some((
						event_id.clone(),
						self.services
							.timeline
							.get_pdu(event_id)
							.await
							.expect("Event disappeared between filtering and fetching"),
					))
				})
				.collect()
				.await;
			assert_eq!(
				state_events.len(),
				res.pdu_ids.len(),
				"Failed to load all required state events despite allegedly knowing all of them \
				 already",
			);
		} else {
			let total_count = res.pdu_ids.len();
			let missing_count = to_fetch.len();
			let missing_threshold = max(50, total_count >> 2);
			if missing_count >= missing_threshold {
				// If there's more than 50 events to fetch, or we're missing 25% or more of the
				// state, we would need to make a lot of atomic requests, so we'll just try
				// to fetch the full state from the remote instead.
				// Since this endpoint might fail in huge rooms, we fall back to atomic fetch
				// anyway.
				warn!(
					elapsed=?start.elapsed(),
					%missing_count,
					%total_count,
					%missing_threshold,
					"Fetching full state from remote server for event"
				);
				let state_response = tokio::time::timeout(
					Duration::from_secs(30),
					self.fetch_full_state(origin, create_event, room_id, event_id),
				)
				.await;
				info!(
					elapsed=?start.elapsed(),
					%missing_count,
					%total_count,
					%missing_threshold,
					"Fetched full state from remote server for event"
				);
				let fetched_state = match state_response {
					| Ok(Ok(state)) => {
						// Filter to ensure we only use the PDUs we were expecting, preventing
						// arbitrary state injection.
						// Atomic fetch does not have this problem as each PDU is evaluated
						// individually.
						let expected = &res.pdu_ids;
						state
							.into_iter()
							.stream()
							.broad_filter_map(|(event_id, pdu)| async move {
								expected.contains(&event_id).then_some((event_id, pdu))
							})
							.collect()
							.await
					},
					| Ok(Err(e)) => {
						warn!(
							elapsed=?start.elapsed(),
							error=?e,
							%origin,
							"Failed to fetch full state from remote, falling back to atomic fetch"
						);
						self.fetch_and_handle_auth_events(
							origin,
							res.pdu_ids.clone(),
							create_event,
							room_id,
						)
						.await
					},
					| Err(e) => {
						warn!(
							elapsed=?start.elapsed(),
							error=?e,
							%origin,
							"Remote did not return room state in an acceptable timeframe, falling back to atomic fetch"
						);
						self.fetch_and_handle_auth_events(
							origin,
							res.pdu_ids.clone(),
							create_event,
							room_id,
						)
						.await
					},
				};

				assert!(
					!fetched_state.is_empty(),
					"fetch_full_state or fetch_and_handle_missing_events returned empty state \
					 map"
				);
				state_events.extend(fetched_state);
			} else {
				state_events = res
					.pdu_ids
					.iter()
					.stream()
					.broad_filter_map(|event_id| async move {
						self.services
							.timeline
							.get_pdu(event_id)
							.await
							.map(|p| (event_id.to_owned(), p))
							.ok()
					})
					.collect()
					.await;
				assert!(
					!state_events.is_empty(),
					"Only missing {} events but read-ahead state vec was empty",
					to_fetch.len()
				);
				debug!(
					elapsed=?start.elapsed(),
					to_fetch = to_fetch.len(),
					"Fetching missing events for state from remote"
				);
				let fetched_state = self
					.fetch_and_handle_auth_events(origin, to_fetch, create_event, room_id)
					.await;
				state_events.extend(fetched_state);
			}
		}
		if state_events.is_empty() {
			return Ok(HashMap::new());
		}

		let mut state: HashMap<ShortStateKey, OwnedEventId> =
			HashMap::with_capacity(state_events.len());
		debug!(elapsed=?start.elapsed(), events = state_events.len(), "Processing state events");
		for (event_id, pdu) in state_events {
			let state_key = pdu.state_key().ok_or_else(|| {
				err!(Request(BadJson("Found non-state pdu in state events: {event_id}")))
			})?;

			let shortstatekey = self
				.services
				.short
				.get_or_create_shortstatekey(&pdu.kind().to_string().into(), state_key)
				.await;

			match state.entry(shortstatekey) {
				| hash_map::Entry::Vacant(v) => {
					v.insert(pdu.event_id().to_owned());
				},
				| hash_map::Entry::Occupied(existing) => {
					return Err!(Request(Forbidden(
						"State event's type and state_key combination exists multiple times \
						 ({event_id} + {}): ({}, \"{}\")",
						existing.get(),
						pdu.kind(),
						state_key,
					)));
				},
			}
		}
		trace!(elapsed=?start.elapsed(), "fetch_state finished");
		Ok(state)
	}

	async fn fetch_state_ids_from_backfill_servers(
		&self,
		event_id: OwnedEventId,
		room_id: OwnedRoomId,
	) -> Result<get_room_state_ids::v1::Response> {
		let candidates = self
			.services
			.timeline
			.candidate_backfill_servers(&room_id)
			.await;
		if candidates.is_empty() {
			return Err!(Request(NotFound(
				"Cannot ask any other servers for the state at this event"
			)));
		}
		debug!(%room_id, ?candidates, "Asking backfill servers for state_ids");
		let futures = candidates.iter().map(|server_name| {
			Box::pin(
				self.services
					.sending
					.send_federation_request(
						server_name,
						get_room_state_ids::v1::Request::new(event_id.clone(), room_id.clone()),
					)
					.inspect_err(|e| {
						debug_warn!("Fallback fetching state for event failed: {e}");
					}),
			)
		});
		Ok(select_ok(futures).await?.0)
	}

	/// Fetches the full state via `GET /_matrix/federation/v1/state` from a
	/// remote server, and persists all the incoming auth chain events and
	/// state events as outliers, for use later.
	///
	/// Any events that cannot be persisted are dropped with a warning.
	pub(super) async fn fetch_full_state(
		&self,
		origin: &ServerName,
		create_event: &PduEvent,
		room_id: &RoomId,
		event_id: &EventId,
	) -> Result<HashMap<OwnedEventId, PduEvent>> {
		let start = Instant::now();
		trace!("Fetching full state from remote server");
		let res: get_room_state::v1::Response = self
			.services
			.sending
			.send_federation_request(
				origin,
				get_room_state::v1::Request::new(event_id.to_owned(), room_id.to_owned()),
			)
			.await
			.inspect_err(|e| debug_warn!("Fetching state for event failed: {e}"))?;
		debug!(elapsed=?start.elapsed(), count = res.auth_chain.len(), "Handling incoming auth chain...");
		res.auth_chain
			.iter()
			.stream()
			.broad_filter_map(|raw_event_json| async {
				if let Some(parsed) = self.parse_incoming_pdu(raw_event_json).await.ok()
					&& parsed.0 == room_id
				{
					Some(parsed)
				} else {
					None
				}
			})
			.for_each_concurrent(
				None,
				|(incoming_room_id, incoming_event_id, incoming_event_json)| async move {
					self.handle_outlier_pdu(
						origin,
						create_event,
						&incoming_event_id,
						&incoming_room_id,
						incoming_event_json,
						true,
					)
					.await
					.inspect_err(|e| {
						warn!(
							%incoming_room_id,
							%incoming_event_id,
							?e,
							"Failed to handle auth chain event from state fetch"
						);
					})
					.ok();
				},
			)
			.await;
		debug!(elapsed=?start.elapsed(), count = res.pdus.len(), "Handling incoming state PDUs...");
		let r = res
			.pdus
			.iter()
			.stream()
			.broad_filter_map(|raw_event_json| async {
				if let Some(parsed) = self.parse_incoming_pdu(raw_event_json).await.ok()
					&& parsed.0 == room_id
				{
					Some(parsed)
				} else {
					None
				}
			})
			.broad_filter_map(
				|(incoming_room_id, incoming_event_id, incoming_event_json)| async move {
					self.handle_outlier_pdu(
						origin,
						create_event,
						&incoming_event_id,
						&incoming_room_id,
						incoming_event_json,
						true,
					)
					.await
					.inspect_err(|e| {
						warn!(
							elapsed=?start.elapsed(),
							%incoming_room_id,
							%incoming_event_id,
							?e,
							"Failed to handle state event from state fetch"
						);
					})
					.ok()
				},
			)
			.fold(HashMap::new(), |mut acc, (event, _)| async move {
				acc.insert(event.event_id().to_owned(), event);
				acc
			})
			.await;
		trace!(elapsed=?start.elapsed(), "fetch_full_state finished");
		Ok(r)
	}
}
