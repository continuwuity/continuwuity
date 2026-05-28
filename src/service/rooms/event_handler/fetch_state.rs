use std::collections::{HashMap, hash_map};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_warn, err, trace, utils::IterStream, warn,
};
use futures::StreamExt;
use ruma::{
	EventId, OwnedEventId, RoomId, ServerName,
	api::federation::event::{get_room_state, get_room_state_ids},
	events::StateEventType,
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
	///
	/// The end result is a result containing a map of shortstatekeys to event
	/// IDs. The underlying `Option` is always `Some`.
	#[tracing::instrument(skip_all)]
	pub(super) async fn fetch_state(
		&self,
		origin: &ServerName,
		create_event: &PduEvent,
		room_id: &RoomId,
		event_id: &EventId,
	) -> Result<Option<HashMap<u64, OwnedEventId>>> {
		trace!(%origin, "Asking remote for state_ids");
		let res: get_room_state_ids::v1::Response = self
			.services
			.sending
			.send_federation_request(
				origin,
				get_room_state_ids::v1::Request::new(event_id.to_owned(), room_id.to_owned()),
			)
			.await
			.inspect_err(|e| debug_warn!("Fetching state for event failed: {e}"))?;

		debug!(events = res.pdu_ids.len(), "Fetching state events");
		let mut state_events: HashMap<OwnedEventId, PduEvent> =
			HashMap::with_capacity(res.pdu_ids.len());
		let to_fetch: Vec<OwnedEventId> = res
			.pdu_ids
			.clone()
			.into_iter()
			.stream()
			.broad_filter_map(|event_id| async move {
				if self.services.timeline.pdu_exists(&event_id).await {
					None
				} else {
					Some(event_id)
				}
			})
			.collect()
			.await;
		if to_fetch.is_empty() {
			debug!("All required state events are already known.");
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
		} else if to_fetch.len() >= 100 {
			// That's a lot of events to fetch, just ask for the full state
			// at that point.
			debug_warn!(
				to_fetch = to_fetch.len(),
				"Fetching full state from remote server for event"
			);
			state_events.extend(
				self.fetch_full_state(origin, create_event, room_id, event_id)
					.await?,
			);
		} else {
			debug!(to_fetch = to_fetch.len(), "Fetching missing events for state from remote");
			state_events.extend(
				self.fetch_and_handle_missing_events(origin, to_fetch, create_event, room_id)
					.await,
			);
		}

		let mut state: HashMap<ShortStateKey, OwnedEventId> =
			HashMap::with_capacity(state_events.len());
		debug!(events = state_events.len(), "Processing state events");
		for (event_id, pdu) in state_events {
			let state_key = pdu.state_key().ok_or_else(|| {
				err!(Database("Found non-state pdu in state events: {event_id}"))
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
				| hash_map::Entry::Occupied(_) => {
					return Err!(Database(
						"State event's type and state_key combination exists multiple times \
						 ({event_id}): {}, {}",
						pdu.kind(),
						state_key
					));
				},
			}
		}

		// The original create event must still be in the state
		let create_shortstatekey = self
			.services
			.short
			.get_shortstatekey(&StateEventType::RoomCreate, "")
			.await?;

		if state.get(&create_shortstatekey).map(AsRef::as_ref) != Some(create_event.event_id()) {
			return Err!(Database("Incoming event refers to wrong create event."));
		}

		Ok(Some(state))
	}

	/// Fetches the full state via `GET /_matrix/federation/v1/state` from a
	/// remote server, and persists all the incoming auth chain events and
	/// state events as outliers, for use later.
	///
	/// Any events that cannot be persisted are dropped with a warning.
	/// TODO: make it noisy?
	pub(super) async fn fetch_full_state(
		&self,
		origin: &ServerName,
		create_event: &PduEvent,
		room_id: &RoomId,
		event_id: &EventId,
	) -> Result<HashMap<OwnedEventId, PduEvent>> {
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
		debug!(count = res.auth_chain.len(), "Handling incoming auth chain...");
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
		debug!(count = res.pdus.len(), "Handling incoming state PDUs...");
		Ok(res
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
			.await)
	}
}
