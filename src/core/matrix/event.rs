mod content;
mod format;
mod redact;
mod type_ext;

use ruma::{
	EventId, MilliSecondsSinceUnixEpoch, OwnedEventId, RoomId, RoomVersionId, UserId,
	events::TimelineEventType,
};
use serde::Deserialize;
use serde_json::{Value as JsonValue, value::RawValue as RawJsonValue};

pub use self::type_ext::TypeExt;
use super::state_key::StateKey;
use crate::Result;

/// Abstraction of a PDU so users can have their own PDU types.
pub trait Event {
	/// Serialize into a Ruma JSON format, consuming.
	#[inline]
	fn into_format<T>(self) -> T
	where
		T: From<format::Owned<Self>>,
		Self: Sized,
	{
		format::Owned(self).into()
	}

	/// Serialize into a Ruma JSON format
	#[inline]
	fn to_format<'a, T>(&'a self) -> T
	where
		T: From<format::Ref<'a, Self>>,
		Self: Sized + 'a,
	{
		format::Ref(self).into()
	}

	#[inline]
	fn get_content_as_value(&self) -> JsonValue
	where
		Self: Sized,
	{
		content::as_value(self)
	}

	#[inline]
	fn get_content<T>(&self) -> Result<T>
	where
		for<'de> T: Deserialize<'de>,
		Self: Sized,
	{
		content::get::<T, _>(self)
	}

	#[inline]
	fn redacts_id(&self, room_version: &RoomVersionId) -> Option<OwnedEventId>
	where
		Self: Sized,
	{
		redact::redacts_id(self, room_version)
	}

	#[inline]
	fn is_redacted(&self) -> bool
	where
		Self: Sized,
	{
		redact::is_redacted(self)
	}

	fn is_owned(&self) -> bool;

	//
	// Canonical properties
	//

	/// All the authenticating events for this event.
	fn auth_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Send + '_;

	/// The event's content.
	fn content(&self) -> &RawJsonValue;

	/// The `EventId` of this event.
	fn event_id(&self) -> &EventId;

	/// The time of creation on the originating server.
	fn origin_server_ts(&self) -> MilliSecondsSinceUnixEpoch;

	/// The events before this event.
	fn prev_events(&self) -> impl DoubleEndedIterator<Item = &EventId> + Send + '_;

	/// If this event is a redaction event this is the event it redacts.
	fn redacts(&self) -> Option<&EventId>;

	/// The `RoomId` of this event.
	fn room_id(&self) -> &RoomId;

	/// The `UserId` of this event.
	fn sender(&self) -> &UserId;

	/// The state key for this event.
	fn state_key(&self) -> Option<&str>;

	/// The event type.
	fn kind(&self) -> &TimelineEventType;

	/// Metadata container; peer-trusted only.
	fn unsigned(&self) -> Option<&RawJsonValue>;

	//#[deprecated]
	#[inline]
	fn event_type(&self) -> &TimelineEventType { self.kind() }
}
