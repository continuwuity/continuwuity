//! Auth checks relevant to any event's `auth_events`.
//!
//! See: https://spec.matrix.org/v1.16/rooms/v12/#authorization-rules
use std::{
	collections::{HashMap, HashSet},
	future::Future,
};

use ruma::{EventId, OwnedEventId, RoomId, events::StateEventType};

use crate::{Event, EventTypeExt, Pdu, RoomVersion, matrix::StateKey, state_res::Error, warn};

// Checks for duplicate auth events in the `auth_events` field of an event.
// Note: the caller should already have all of the auth events fetched.
//
// If there are multiple auth events of the same type and state key, this
// returns an error. Otherwise, it returns a map of (type, state_key) to the
// corresponding auth event.
pub async fn check_duplicate_auth_events<E, Fut>(
	auth_events: &[OwnedEventId],
	fetch_event: impl Fn(&EventId) -> Fut + Send,
) -> Result<HashMap<(StateEventType, StateKey), E>, Error>
where
	Fut: Future<Output = Result<Option<E>, Error>> + Send,
	E: Event + Send + Sync,
	for<'a> &'a E: Event + Send,
{
	let mut seen: HashMap<(StateEventType, StateKey), E> = HashMap::new();

	// Considering all of the event's auth events:
	for auth_event_id in auth_events {
		if let Ok(Some(auth_event)) = fetch_event(auth_event_id).await {
			let event_type = auth_event.kind();
			// If this is not a state event, reject it.
			let Some(state_key) = &auth_event.state_key() else {
				return Err(Error::InvalidPdu(format!(
					"Auth event {:?} is not a state event",
					auth_event_id
				)));
			};
			let type_key_pair: (StateEventType, StateKey) =
				event_type.clone().with_state_key(state_key.clone());

			// If there are duplicate entries for a given type and state_key pair, reject.
			if seen.contains_key(&type_key_pair) {
				return Err(Error::DuplicateAuthEvents(format!(
					"({:?},\"{:?}\")",
					event_type, state_key
				)));
			}
			seen.insert(type_key_pair, auth_event);
		} else {
			return Err(Error::NotFound(auth_event_id.as_str().to_owned()));
		}
	}

	Ok(seen)
}

// Checks that the event does not refer to any auth events that it does not need
// to.
pub fn check_unnecessary_auth_events(
	auth_events: &HashSet<(StateEventType, StateKey)>,
	expected: &Vec<(StateEventType, StateKey)>,
) -> Result<(), Error> {
	// If there are entries whose type and state_key don't match those specified by
	// the auth events selection algorithm described in the server specification,
	// reject.
	let remaining = auth_events
		.iter()
		.filter(|key| !expected.contains(key))
		.collect::<HashSet<_>>();
	if !remaining.is_empty() {
		return Err(Error::UnselectedAuthEvents(format!("{:?}", remaining)));
	}
	Ok(())
}

// Checks that all provided auth events were not rejected previously.
//
// TODO: this is currently a no-op and always returns Ok(()).
pub fn check_all_auth_events_accepted<E>(
	_auth_events: &HashMap<(StateEventType, StateKey), E>,
) -> Result<(), Error>
where
	E: Event + Send + Sync,
	for<'a> &'a E: Event + Send,
{
	Ok(())
}

// Checks that all auth events are from the same room as the event being
// validated.
pub fn check_auth_same_room<E>(auth_events: &Vec<E>, room_id: &RoomId) -> bool
where
	E: Event + Send + Sync,
	for<'a> &'a E: Event + Send,
{
	for auth_event in auth_events {
		if let Some(auth_room_id) = &auth_event.room_id() {
			if auth_room_id.as_str() != room_id.as_str() {
				warn!(
					auth_event_id=%auth_event.event_id(),
					"Auth event room id {} does not match expected room id {}",
					auth_room_id,
					room_id
				);
				return false;
			}
		} else {
			warn!(auth_event_id=%auth_event.event_id(), "Auth event has no room_id");
			return false;
		}
	}
	true
}

// Performs all auth event checks for the given event.
pub async fn check_auth_events<E, Fut>(
	event: &Pdu,
	room_id: &RoomId,
	room_version: &RoomVersion,
	fetch_event: impl Fn(&EventId) -> Fut + Send,
) -> Result<HashMap<(StateEventType, StateKey), E>, Error>
where
	Fut: Future<Output = Result<Option<E>, Error>> + Send,
	E: Event + Send + Sync,
	for<'a> &'a E: Event + Send,
{
	// If there are duplicate entries for a given type and state_key pair, reject.
	let auth_events_map = check_duplicate_auth_events(&event.auth_events, fetch_event).await?;
	let auth_events_set: HashSet<(StateEventType, StateKey)> =
		auth_events_map.keys().cloned().collect();

	// If there are entries whose type and state_key don’t match those specified by
	// the auth events selection algorithm described in the server specification,
	// reject.
	let expected_auth_events = crate::state_res::auth_types_for_event(
		event.kind(),
		event.sender(),
		event.state_key(),
		event.content(),
		room_version,
	)?;
	if let Err(e) = check_unnecessary_auth_events(&auth_events_set, &expected_auth_events) {
		return Err(e);
	}

	// If there are entries which were themselves rejected under the checks
	// performed on receipt of a PDU, reject.
	if let Err(e) = check_all_auth_events_accepted(&auth_events_map) {
		return Err(e);
	}

	// If any event in auth_events has a room_id which does not match that of the
	// event being authorised, reject.
	let auth_event_refs: Vec<E> = auth_events_map.values().cloned().collect();
	if !check_auth_same_room(&auth_event_refs, room_id) {
		return Err(Error::InvalidPdu(
			"One or more auth events are from a different room".to_owned(),
		));
	}

	Ok(auth_events_map)
}
