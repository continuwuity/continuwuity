use std::borrow::Borrow;

use conduwuit::{
	Result, err, implement,
	matrix::{Event, StateKey},
};
use futures::{Stream, StreamExt, TryFutureExt};
use ruma::{EventId, RoomId, events::StateEventType};
use serde::Deserialize;

/// Returns a single PDU from `room_id` with key (`event_type`,`state_key`).
#[implement(super::Service)]
pub async fn room_state_get_content<T>(
	&self,
	room_id: &RoomId,
	event_type: &StateEventType,
	state_key: &str,
) -> Result<T>
where
	T: for<'de> Deserialize<'de>,
{
	self.room_state_get(room_id, event_type, state_key)
		.await
		.and_then(|event| event.get_content())
}

/// Returns the full room state.
#[implement(super::Service)]
#[tracing::instrument(skip(self), level = "debug")]
pub fn room_state_full<'a>(
	&'a self,
	room_id: &'a RoomId,
) -> impl Stream<Item = Result<((StateEventType, StateKey), impl Event)>> + Send + 'a {
	self.services
		.state
		.get_room_shortstatehash(room_id)
		.map_ok(|shortstatehash| self.state_full(shortstatehash).map(Ok).boxed())
		.map_err(move |e| err!(Database("Missing state for {room_id:?}: {e:?}")))
		.try_flatten_stream()
}

/// Returns the full room state pdus
#[implement(super::Service)]
#[tracing::instrument(skip(self), level = "debug")]
pub fn room_state_full_pdus<'a>(
	&'a self,
	room_id: &'a RoomId,
) -> impl Stream<Item = Result<impl Event>> + Send + 'a {
	self.services
		.state
		.get_room_shortstatehash(room_id)
		.map_ok(|shortstatehash| self.state_full_pdus(shortstatehash).map(Ok).boxed())
		.map_err(move |e| err!(Database("Missing state for {room_id:?}: {e:?}")))
		.try_flatten_stream()
}

/// Returns a single EventId from `room_id` with key (`event_type`,
/// `state_key`).
#[implement(super::Service)]
#[tracing::instrument(skip(self), level = "debug")]
pub async fn room_state_get_id<Id>(
	&self,
	room_id: &RoomId,
	event_type: &StateEventType,
	state_key: &str,
) -> Result<Id>
where
	Id: for<'de> Deserialize<'de> + Sized + ToOwned,
	<Id as ToOwned>::Owned: Borrow<EventId>,
{
	self.services
		.state
		.get_room_shortstatehash(room_id)
		.and_then(|shortstatehash| self.state_get_id(shortstatehash, event_type, state_key))
		.await
}

/// Returns a single PDU from `room_id` with key (`event_type`,
/// `state_key`).
#[implement(super::Service)]
#[tracing::instrument(skip(self), level = "debug")]
pub async fn room_state_get(
	&self,
	room_id: &RoomId,
	event_type: &StateEventType,
	state_key: &str,
) -> Result<impl Event> {
	self.services
		.state
		.get_room_shortstatehash(room_id)
		.and_then(|shortstatehash| self.state_get(shortstatehash, event_type, state_key))
		.await
}

/// Returns all state keys for the given `room_id` and `event_type`.
#[implement(super::Service)]
#[tracing::instrument(skip(self), level = "debug")]
pub async fn room_state_keys(
	&self,
	room_id: &RoomId,
	event_type: &StateEventType,
) -> Result<Vec<String>> {
	let shortstatehash = self.services.state.get_room_shortstatehash(room_id).await?;

	let state_keys: Vec<String> = self
		.state_keys(shortstatehash, event_type)
		.map(|state_key| state_key.to_string())
		.collect()
		.await;

	Ok(state_keys)
}
