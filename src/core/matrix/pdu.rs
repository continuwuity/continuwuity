mod count;
pub mod hashes;
mod id;
pub mod metadata;
mod partial;
mod raw_id;
mod redact;
#[cfg(test)]
mod tests;
mod unsigned;
pub mod v2;
pub mod v3;

use std::cmp::Ordering;

use assign::assign;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId,
	OwnedRoomId, OwnedServerName, OwnedUserId, RoomId, ServerSignatures, UInt, UserId, event_id,
	events::TimelineEventType, room_version_rules::RoomVersionRules, state_res::Event,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::value::RawValue as RawJsonValue;

pub use self::{
	Count as PduCount, Id as PduId, Pdu as PduEvent, RawId as RawPduId,
	count::Count,
	id::{ShortId, *},
	partial::PartialPdu,
	raw_id::*,
};
use super::StateKey;
use crate::{
	Err, Result,
	pdu::{hashes::Hashes, metadata::PduMetadata},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventFormatVersion {
	/// V1 represents the PDU format for rooms v1 and v2.
	V1,
	/// V2 represents the PDU format for rooms v3-v11.
	V2,
	/// V3 represents the PDU format for rooms v12+.
	V3,
}

impl From<RoomVersionRules> for EventFormatVersion {
	fn from(rules: RoomVersionRules) -> Self {
		// TODO: this is hacky because ruma doesn't actually define event versions in an
		// enum, but does define consts with the appropriate rules.
		if rules.event_format.require_event_id {
			Self::V1
		} else if rules.event_format.require_room_create_room_id {
			Self::V2
		} else {
			Self::V3
		}
	}
}

/// Trait that allows conversion from a generic PDU to a concrete PDU type.
pub trait ConcretePDU: ruma::state_res::Event<Id = OwnedEventId> {
	/// The concrete type for this PDU (`v2::PDU`, `v3::PDU`, etc)
	type Concrete;

	/// Get a reference to the concrete PDU type.
	fn to_concrete(&self) -> &Self::Concrete;

	/// Get the value of the concrete PDU type, consuming.
	fn into_concrete(self) -> Self::Concrete;
}

pub fn new_pdu<E, C, P, JSON>(
	format: EventFormatVersion,
	metadata: PduMetadata,
	json: serde_json::Value,
) -> Result<E>
where
	E: ConcretePDU<Concrete = C>,
	JSON: DeserializeOwned,
{
	match format {
		| EventFormatVersion::V2 =>
			Ok(assign!(v2::PDU::try_from(json)?, {internal_metadata: metadata})),
		| EventFormatVersion::V3 =>
			Ok(assign!(v3::PDU::try_from(json)?, {internal_metadata: metadata})),
		| _ => Err!("Unsupported event format: {format:?}"),
	}
}

/// A builder type that specifies every field present on any PDU type, allowing
/// it to be used to create any PDU version with the appropriate function.
///
/// See `create_pdu`.
#[derive(Clone, Debug, Default)]
pub struct CommonPDUBuilder {
	pub auth_events: Option<Vec<OwnedEventId>>,
	pub content: Option<Box<RawJsonValue>>,
	pub depth: Option<UInt>,
	pub hashes: Option<Hashes>,
	pub origin_server_ts: Option<MilliSecondsSinceUnixEpoch>,
	pub prev_events: Option<Vec<OwnedEventId>>,
	pub redacts: Option<OwnedEventId>,
	pub room_id: Option<OwnedRoomId>,
	pub sender: Option<OwnedUserId>,
	pub signatures: Option<ServerSignatures>,
	pub state_key: Option<String>,
	pub event_type: Option<TimelineEventType>,
	pub unsigned: Option<Box<RawJsonValue>>,
	pub internal_metadata: Option<PduMetadata>,
}

/// Creates a new PDU based on the given version and builder.
///
/// ## Example
///
/// ```rust
/// use conduwuit_core::pdu::{CommonPDUBuilder, create_pdu}
///
/// let room_version_rules = RoomVersionRules::V10;  // This usually comes dynamically.
/// let builder = CommonPDUBuilder {
///     auth_events: Some(vec![event_id!("$auth_event_id").to_owned()]),
///     content: Some(Box::new(serde_json::json!({"key": "value"}))),
///     depth: Some(1),
///     origin_server_ts: Some(MilliSecondsSinceUnixEpoch(123456789)),
///     prev_events: Some(vec![event_id!("$prev_event_id").to_owned()]),
///     room_id: Some(room_id!("!room_id").to_owned()),
///     sender: Some(user_id!("@sender:example.com").to_owned()),
///     event_type: Some(TimelineEventType::RoomMessage),
///     ..Default::default()
/// };
/// let pdu = create_pdu(
///     room_version_rules.into(),
///     builder,
/// ).expect("PDU creation must succeed");
/// ```
pub fn create_pdu<C>(version: EventFormatVersion, builder: CommonPDUBuilder) -> Result<C> {
	match version {
		| EventFormatVersion::V2 => Ok(create_v2_pdu(builder)),
		| EventFormatVersion::V3 => Ok(create_v3_pdu(builder)),
		| _ => Err!("Unsupported event format: {version:?}"),
	}
}

fn create_v2_pdu<C>(builder: CommonPDUBuilder) -> v2::PDU {
	v2::PDU {
		auth_events: builder
			.auth_events
			.expect("auth_events must be provided for a V2 PDU"),
		content: builder
			.content
			.expect("content must be provided for a V2 PDU"),
		depth: builder.depth.expect("depth must be provided for a V2 PDU"),
		hashes: builder.hashes.unwrap_or_default(), /* We allow default here since we might not
		                                             * have hashed yet */
		origin_server_ts: builder
			.origin_server_ts
			.expect("origin_server_ts must be provided for a V2 PDU"),
		prev_events: builder
			.prev_events
			.expect("prev_events must be provided for a V2 PDU"),
		redacts: builder.redacts,
		room_id: builder
			.room_id
			.expect("room_id must be provided for a V2 PDU"),
		sender: builder
			.sender
			.expect("sender must be provided for a V2 PDU"),
		signatures: builder.signatures.unwrap_or_default(), /* We allow default here since we
		                                                     * might not have signed yet */
		state_key: builder.state_key,
		event_type: builder
			.event_type
			.expect("event_type must be provided for a V2 PDU"),
		unsigned: builder.unsigned,
		internal_metadata: builder
			.internal_metadata
			.unwrap_or_else(|| PduMetadata::new(event_id!("$unknown").to_owned())),
	}
}

fn create_v3_pdu(builder: CommonPDUBuilder) -> v3::PDU {
	v3::PDU {
		auth_events: builder
			.auth_events
			.expect("auth_events must be provided for a V3 PDU"),
		content: builder
			.content
			.expect("content must be provided for a V3 PDU"),
		depth: builder.depth.expect("depth must be provided for a V3 PDU"),
		hashes: builder.hashes.unwrap_or_default(), /* We allow default here since we might not
		                                             * have hashed yet */
		origin_server_ts: builder
			.origin_server_ts
			.expect("origin_server_ts must be provided for a V3 PDU"),
		prev_events: builder
			.prev_events
			.expect("prev_events must be provided for a V3 PDU"),
		room_id: builder.room_id,
		sender: builder
			.sender
			.expect("sender must be provided for a V3 PDU"),
		signatures: builder.signatures.unwrap_or_default(), /* We allow default here since we
		                                                     * might not have signed yet */
		state_key: builder.state_key,
		event_type: builder
			.event_type
			.expect("event_type must be provided for a V3 PDU"),
		unsigned: builder.unsigned,
		internal_metadata: builder
			.internal_metadata
			.unwrap_or_else(|| PduMetadata::new(event_id!("$unknown").to_owned())),
	}
}

/// Persistent Data Unit (Event)
#[deprecated(note = "Use `v2::PDU` or `v3::PDU` or `ruma::state_res::Event` instead")]
#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct Pdu {
	pub event_id: OwnedEventId,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub room_id: Option<OwnedRoomId>,

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

impl crate::Event for Pdu {
	#[inline]
	fn auth_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Clone + Send + '_ {
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
	fn prev_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Clone + Send + '_ {
		self.prev_events.iter().map(AsRef::as_ref)
	}

	#[inline]
	fn redacts(&self) -> Option<&EventId> { self.redacts.as_deref() }

	#[inline]
	fn room_id(&self) -> Option<&RoomId> { self.room_id.as_deref() }

	#[inline]
	fn room_id_or_hash(&self) -> OwnedRoomId {
		if *self.event_type() != TimelineEventType::RoomCreate {
			return self
				.room_id()
				.expect("Event must have a room ID")
				.to_owned();
		}
		if let Some(room_id) = &self.room_id {
			// v1-v11
			room_id.clone()
		} else {
			// v12+
			let constructed_hash = self.event_id.as_str().replace('$', "!");
			RoomId::parse(&constructed_hash).expect("event ID can be parsed")
		}
	}

	#[inline]
	fn sender(&self) -> &UserId { &self.sender }

	#[inline]
	fn state_key(&self) -> Option<&str> { self.state_key.as_deref() }

	#[inline]
	fn kind(&self) -> &TimelineEventType { &self.kind }

	#[inline]
	fn unsigned(&self) -> Option<&RawJsonValue> { self.unsigned.as_deref() }

	#[inline]
	fn as_mut_pdu(&mut self) -> &mut Pdu { self }

	#[inline]
	fn as_pdu(&self) -> &Pdu { self }

	#[inline]
	fn into_pdu(self) -> Pdu { self }
}

impl crate::Event for &Pdu {
	#[inline]
	fn auth_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Clone + Send + '_ {
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
	fn prev_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Clone + Send + '_ {
		self.prev_events.iter().map(AsRef::as_ref)
	}

	#[inline]
	fn redacts(&self) -> Option<&EventId> { self.redacts.as_deref() }

	#[inline]
	fn room_id(&self) -> Option<&RoomId> { self.room_id.as_ref().map(AsRef::as_ref) }

	#[inline]
	fn room_id_or_hash(&self) -> OwnedRoomId {
		if *self.event_type() != TimelineEventType::RoomCreate {
			return self
				.room_id()
				.expect("Event must have a room ID")
				.to_owned();
		}
		if let Some(room_id) = &self.room_id {
			// v1-v11
			room_id.clone()
		} else {
			// v12+
			let constructed_hash = self.event_id.as_str().replace('$', "!");
			RoomId::parse(&constructed_hash).expect("event ID can be parsed")
		}
	}

	#[inline]
	fn sender(&self) -> &UserId { &self.sender }

	#[inline]
	fn state_key(&self) -> Option<&str> { self.state_key.as_deref() }

	#[inline]
	fn kind(&self) -> &TimelineEventType { &self.kind }

	#[inline]
	fn unsigned(&self) -> Option<&RawJsonValue> { self.unsigned.as_deref() }

	#[inline]
	fn as_pdu(&self) -> &Pdu { self }

	#[inline]
	fn into_pdu(self) -> Pdu { self.clone() }
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
