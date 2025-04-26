mod builder;
mod content;
mod count;
mod event_id;
mod filter;
mod id;
mod raw_id;
mod redact;
mod relation;
#[cfg(test)]
mod tests;
mod unsigned;

use std::cmp::Ordering;

use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId,
	OwnedRoomId, OwnedServerName, OwnedUserId, RoomId, UInt, UserId, events::TimelineEventType,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue as RawJsonValue;

pub use self::{
	Count as PduCount, Id as PduId, Pdu as PduEvent, RawId as RawPduId,
	builder::{Builder, Builder as PduBuilder},
	count::Count,
	event_id::*,
	id::{ShortId, *},
	raw_id::*,
};
use super::{Event, StateKey};
use crate::Result;

/// Persistent Data Unit (Event)
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Pdu {
	pub event_id: OwnedEventId,

	pub room_id: OwnedRoomId,

	pub sender: OwnedUserId,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub origin: Option<OwnedServerName>,

	pub origin_server_ts: UInt,

	#[serde(rename = "type")]
	pub kind: TimelineEventType,

	pub content: Box<RawJsonValue>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub state_key: Option<StateKey>,

	pub prev_events: Vec<OwnedEventId>,

	pub depth: UInt,

	pub auth_events: Vec<OwnedEventId>,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub redacts: Option<OwnedEventId>,

	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub unsigned: Option<Box<RawJsonValue>>,

	pub hashes: EventHash,

	// BTreeMap<Box<ServerName>, BTreeMap<ServerSigningKeyId, String>>
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub signatures: Option<Box<RawJsonValue>>,
}

/// Content hashes of a PDU.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventHash {
	/// The SHA-256 hash.
	pub sha256: String,
}

impl Pdu {
	pub fn from_id_val(event_id: &EventId, mut json: CanonicalJsonObject) -> Result<Self> {
		let event_id = CanonicalJsonValue::String(event_id.into());
		json.insert("event_id".into(), event_id);
		serde_json::to_value(json)
			.and_then(serde_json::from_value)
			.map_err(Into::into)
	}
}

impl Event for Pdu {
	#[inline]
	fn auth_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Send + '_ {
		self.auth_events.iter().map(AsRef::as_ref)
	}

	#[inline]
	fn content(&self) -> &RawJsonValue { &self.content }

	#[inline]
	fn event_id(&self) -> &EventId { &self.event_id }

	#[inline]
	fn origin_server_ts(&self) -> MilliSecondsSinceUnixEpoch {
		MilliSecondsSinceUnixEpoch(self.origin_server_ts)
	}

	#[inline]
	fn prev_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Send + '_ {
		self.prev_events.iter().map(AsRef::as_ref)
	}

	#[inline]
	fn redacts(&self) -> Option<&EventId> { self.redacts.as_deref() }

	#[inline]
	fn room_id(&self) -> &RoomId { &self.room_id }

	#[inline]
	fn sender(&self) -> &UserId { &self.sender }

	#[inline]
	fn state_key(&self) -> Option<&str> { self.state_key.as_deref() }

	#[inline]
	fn kind(&self) -> &TimelineEventType { &self.kind }

	#[inline]
	fn unsigned(&self) -> Option<&RawJsonValue> { self.unsigned.as_deref() }

	#[inline]
	fn is_owned(&self) -> bool { true }
}

impl Event for &Pdu {
	#[inline]
	fn auth_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Send + '_ {
		self.auth_events.iter().map(AsRef::as_ref)
	}

	#[inline]
	fn content(&self) -> &RawJsonValue { &self.content }

	#[inline]
	fn event_id(&self) -> &EventId { &self.event_id }

	#[inline]
	fn origin_server_ts(&self) -> MilliSecondsSinceUnixEpoch {
		MilliSecondsSinceUnixEpoch(self.origin_server_ts)
	}

	#[inline]
	fn prev_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Send + '_ {
		self.prev_events.iter().map(AsRef::as_ref)
	}

	#[inline]
	fn redacts(&self) -> Option<&EventId> { self.redacts.as_deref() }

	#[inline]
	fn room_id(&self) -> &RoomId { &self.room_id }

	#[inline]
	fn sender(&self) -> &UserId { &self.sender }

	#[inline]
	fn state_key(&self) -> Option<&str> { self.state_key.as_deref() }

	#[inline]
	fn kind(&self) -> &TimelineEventType { &self.kind }

	#[inline]
	fn unsigned(&self) -> Option<&RawJsonValue> { self.unsigned.as_deref() }

	#[inline]
	fn is_owned(&self) -> bool { false }
}

/// Prevent derived equality which wouldn't limit itself to event_id
impl Eq for Pdu {}

/// Equality determined by the Pdu's ID, not the memory representations.
impl PartialEq for Pdu {
	fn eq(&self, other: &Self) -> bool { self.event_id == other.event_id }
}

/// Ordering determined by the Pdu's ID, not the memory representations.
impl Ord for Pdu {
	fn cmp(&self, other: &Self) -> Ordering { self.event_id.cmp(&other.event_id) }
}

/// Ordering determined by the Pdu's ID, not the memory representations.
impl PartialOrd for Pdu {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
