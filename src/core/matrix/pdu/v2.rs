use ruma::{
	CanonicalJsonObject, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId,
	OwnedUserId, RoomId, ServerSignatures, UInt, UserId, events::TimelineEventType,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue as RawJsonValue;

use crate::{
	Pdu,
	pdu::{hashes::Hashes, metadata::PduMetadata},
};

/// Represents a persistent data unit (PDU) version 2, introduced in room
/// version 3, up until room version 11.
///
/// Spec: https://spec.matrix.org/v1.19/rooms/v3/#event-format
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PDU {
	/// The event IDs which authorise this event. Between 0 and 10 entries.
	pub auth_events: Vec<OwnedEventId>,
	/// This event's content.
	pub content: Box<RawJsonValue>,
	/// The depth of this event.
	pub depth: UInt,
	/// The content hashes for this event.
	pub hashes: Hashes,
	/// Timestamp in milliseconds on origin homeserver when this event was
	/// created.
	pub origin_server_ts: MilliSecondsSinceUnixEpoch,
	/// The event IDs which immediately precede this event. Between 0 and 20
	/// entries.
	pub prev_events: Vec<OwnedEventId>,
	/// For redaction events, the ID of the event being redacted.
	///
	/// Not used in v11.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub redacts: Option<OwnedEventId>,
	/// The room in which this event resides.
	pub room_id: OwnedRoomId,
	/// The user who sent this event.
	pub sender: OwnedUserId,
	/// The signatures on this event.
	pub signatures: ServerSignatures,
	/// The state key for this PDU, if any.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub state_key: Option<String>,
	/// The type of this PDU.
	#[serde(rename = "type")]
	pub event_type: TimelineEventType,
	/// Additional data added by the origin server but not covered by the
	/// signatures.
	///
	/// This should be `None` when receiving from and transmitting over
	/// federation.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub unsigned: Option<Box<RawJsonValue>>,

	/// Internal metadata that is important to the PDU but should not be
	/// serialized into the final event.
	///
	/// This field MUST NOT be sent to clients or federation.
	pub internal_metadata: PduMetadata,
}

impl PDU {
	#[must_use]
	pub fn internal_metadata(&self) -> &PduMetadata { &self.internal_metadata }
}

impl crate::pdu::ConcretePDU for PDU {
	type Concrete = PDU;

	fn to_concrete(&self) -> &Self::Concrete { &self }

	fn into_concrete(self) -> Self::Concrete { self }
}

impl ruma::state_res::Event for PDU {
	type Id = OwnedEventId;

	fn event_id(&self) -> &Self::Id { &self.internal_metadata.event_id }

	fn room_id(&self) -> Option<&RoomId> { Some(&self.room_id) }

	fn sender(&self) -> &UserId { &self.sender }

	fn origin_server_ts(&self) -> MilliSecondsSinceUnixEpoch { self.origin_server_ts }

	fn event_type(&self) -> &TimelineEventType { &self.event_type }

	fn content(&self) -> &RawJsonValue { self.content.as_ref() }

	fn state_key(&self) -> Option<&str> { self.state_key.as_deref() }

	fn prev_events(&self) -> Box<dyn DoubleEndedIterator<Item = &Self::Id> + '_> {
		Box::new(self.prev_events.iter())
	}

	fn auth_events(&self) -> Box<dyn DoubleEndedIterator<Item = &Self::Id> + '_> {
		Box::new(self.auth_events.iter())
	}

	fn redacts(&self) -> Option<&Self::Id> { self.redacts.as_ref() }

	fn rejected(&self) -> bool { self.internal_metadata().rejected }
}

impl crate::Event for PDU {
	fn as_pdu(&self) -> &Pdu { todo!() }

	fn into_pdu(self) -> Pdu { todo!() }

	fn auth_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Clone + Send + '_ {
		self.auth_events.iter().map(AsRef::as_ref)
	}

	fn content(&self) -> &RawJsonValue { self.content.as_ref() }

	fn event_id(&self) -> &EventId { &self.internal_metadata.event_id }

	fn origin_server_ts(&self) -> MilliSecondsSinceUnixEpoch { self.origin_server_ts }

	fn prev_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Clone + Send + '_ {
		self.prev_events.iter().map(AsRef::as_ref)
	}

	fn redacts(&self) -> Option<&EventId> { self.redacts.as_deref() }

	fn room_id(&self) -> Option<&RoomId> { Some(&self.room_id) }

	fn room_id_or_hash(&self) -> OwnedRoomId { self.room_id.clone() }

	fn sender(&self) -> &UserId { &self.sender }

	fn state_key(&self) -> Option<&str> { self.state_key.as_deref() }

	fn kind(&self) -> &TimelineEventType { self.event_type() }

	fn unsigned(&self) -> Option<&RawJsonValue> { self.unsigned.as_deref() }

	fn event_type(&self) -> &TimelineEventType { &self.event_type }
}

impl TryFrom<&RawJsonValue> for PDU {
	type Error = crate::Error;

	fn try_from(value: &RawJsonValue) -> Result<Self, Self::Error> {
		serde_json::from_str(value.get()).map_err(Into::into)
	}
}

impl TryFrom<Box<RawJsonValue>> for PDU {
	type Error = crate::Error;

	fn try_from(value: Box<RawJsonValue>) -> Result<Self, Self::Error> {
		Self::try_from(value.as_ref())
	}
}

impl TryFrom<serde_json::Value> for PDU {
	type Error = crate::Error;

	fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
		serde_json::from_value(value).map_err(Into::into)
	}
}

impl TryFrom<CanonicalJsonObject> for PDU {
	type Error = crate::Error;

	fn try_from(value: CanonicalJsonObject) -> Result<Self, Self::Error> {
		serde_json::from_value(serde_json::to_value(value)?).map_err(Into::into)
	}
}
