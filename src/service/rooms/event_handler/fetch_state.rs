use std::collections::{HashMap, hash_map};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_warn, err, implement, utils::IterStream,
};
use futures::StreamExt;
use ruma::{
	EventId, OwnedEventId, RoomId, ServerName, api::federation::event::get_room_state_ids,
	events::StateEventType,
};

use crate::{conduwuit::utils::stream::BroadbandExt, rooms::short::ShortStateKey};

/// Call /state_ids to find out what the state at this pdu is. We trust the
/// server's response to some extend (sic), but we still do a lot of checks
/// on the events
#[implement(super::Service)]
#[tracing::instrument(
	level = "debug",
	skip_all,
	fields(%origin),
)]
pub(super) async fn fetch_state<Pdu>(
	&self,
	origin: &ServerName,
	create_event: &Pdu,
	room_id: &RoomId,
	event_id: &EventId,
) -> Result<Option<HashMap<u64, OwnedEventId>>>
where
	Pdu: Event + Send + Sync,
{
	let res: get_room_state_ids::v1::Response = self
		.services
		.sending
		.send_federation_request(
			origin,
			get_room_state_ids::v1::Request::new(event_id.to_owned(), room_id.to_owned()),
		)
		.await
		.inspect_err(|e| debug_warn!("Fetching state for event failed: {e}"))?;

	debug!("Fetching state events");
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
	if !to_fetch.is_empty() {
		if to_fetch.len() >= 100 {
			// That's a lot of events to fetch, just ask for the full state at
			// that point. TODO: fetch /state
		}
		state_events.extend(
			self.fetch_and_handle_missing_events(origin, to_fetch, create_event, room_id)
				.await,
		);
	}

	let mut state: HashMap<ShortStateKey, OwnedEventId> =
		HashMap::with_capacity(state_events.len());
	for (event_id, pdu) in state_events {
		let state_key = pdu
			.state_key()
			.ok_or_else(|| err!(Database("Found non-state pdu in state events: {event_id}")))?;

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
