use ruma::{
	CanonicalJsonObject, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId,
	OwnedUserId, RoomId, ServerSignatures, UInt, UserId,
	events::{StateEventType, TimelineEventType},
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue as RawJsonValue;

use crate::{
	Pdu,
	pdu::{hashes::Hashes, metadata::PduMetadata},
};

/// Represents a persistent data unit (PDU) version 3, introduced in room
/// version 12.
///
/// Spec: https://spec.matrix.org/v1.19/rooms/v12/#event-format
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
	/// The room in which this event resides.
	/// May be None if the event is a create event.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub room_id: Option<OwnedRoomId>,
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

impl crate::pdu::ConcretePDU<PDU> for PDU {
	fn to_concrete(&self) -> &PDU { &self }

	fn into_concrete(self) -> PDU { self }
}

impl ruma::state_res::Event for PDU {
	type Id = OwnedEventId;

	fn event_id(&self) -> &Self::Id { &self.internal_metadata.event_id }

	fn room_id(&self) -> Option<&RoomId> { self.room_id.as_deref() }

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

	fn redacts(&self) -> Option<&Self::Id> { None }

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

	fn redacts(&self) -> Option<&EventId> { None }

	fn room_id(&self) -> Option<&RoomId> { self.room_id.as_deref() }

	fn room_id_or_hash(&self) -> OwnedRoomId {
		if self.event_type == StateEventType::RoomCreate.into()
			&& self.state_key().is_some_and(str::is_empty)
		{
			// Calculate the room ID for a create event.
			RoomId::new_v2(
				self.internal_metadata()
					.event_id
					.as_str()
					.strip_prefix("$")
					.expect("event ID must start with $ sigil"),
			)
			.expect("must be able to create a room ID from create event reference hash")
		} else {
			// Otherwise, all other events have a room ID field.
			self.room_id.clone().unwrap()
		}
	}

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
