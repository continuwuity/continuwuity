use ruma::{
	events::{
		AnyMessageLikeEvent, AnyStateEvent, AnyStrippedStateEvent, AnySyncStateEvent,
		AnySyncTimelineEvent, AnyTimelineEvent, StateEvent, room::member::RoomMemberEventContent,
		space::child::HierarchySpaceChildEvent,
	},
	serde::Raw,
};
use serde_json::json;

use super::{Event, redact};

pub struct Owned<E: Event>(pub(super) E);

pub struct Ref<'a, E: Event>(pub(super) &'a E);

impl<E: Event> From<Owned<E>> for Raw<AnySyncTimelineEvent> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<AnySyncTimelineEvent> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let (redacts, content) = redact::copy(event);
		let mut json = json!({
			"content": content,
			"event_id": event.event_id(),
			"origin_server_ts": event.origin_server_ts(),
			"sender": event.sender(),
			"type": event.event_type(),
		});

		if let Some(redacts) = redacts {
			json["redacts"] = json!(redacts);
		}
		if let Some(state_key) = event.state_key() {
			json["state_key"] = json!(state_key);
		}
		if let Some(unsigned) = event.unsigned() {
			json["unsigned"] = json!(unsigned);
		}

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}

impl<E: Event> From<Owned<E>> for Raw<AnyTimelineEvent> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<AnyTimelineEvent> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let (redacts, content) = redact::copy(event);
		let mut json = json!({
			"content": content,
			"event_id": event.event_id(),
			"origin_server_ts": event.origin_server_ts(),
			"room_id": event.room_id(),
			"sender": event.sender(),
			"type": event.kind(),
		});

		if let Some(redacts) = redacts {
			json["redacts"] = json!(redacts);
		}
		if let Some(state_key) = event.state_key() {
			json["state_key"] = json!(state_key);
		}
		if let Some(unsigned) = event.unsigned() {
			json["unsigned"] = json!(unsigned);
		}

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}

impl<E: Event> From<Owned<E>> for Raw<AnyMessageLikeEvent> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<AnyMessageLikeEvent> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let (redacts, content) = redact::copy(event);
		let mut json = json!({
			"content": content,
			"event_id": event.event_id(),
			"origin_server_ts": event.origin_server_ts(),
			"room_id": event.room_id(),
			"sender": event.sender(),
			"type": event.kind(),
		});

		if let Some(redacts) = &redacts {
			json["redacts"] = json!(redacts);
		}
		if let Some(state_key) = event.state_key() {
			json["state_key"] = json!(state_key);
		}
		if let Some(unsigned) = event.unsigned() {
			json["unsigned"] = json!(unsigned);
		}

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}

impl<E: Event> From<Owned<E>> for Raw<AnyStateEvent> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<AnyStateEvent> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let mut json = json!({
			"content": event.content(),
			"event_id": event.event_id(),
			"origin_server_ts": event.origin_server_ts(),
			"room_id": event.room_id(),
			"sender": event.sender(),
			"state_key": event.state_key(),
			"type": event.kind(),
		});

		if let Some(unsigned) = event.unsigned() {
			json["unsigned"] = json!(unsigned);
		}

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}

impl<E: Event> From<Owned<E>> for Raw<AnySyncStateEvent> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<AnySyncStateEvent> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let mut json = json!({
			"content": event.content(),
			"event_id": event.event_id(),
			"origin_server_ts": event.origin_server_ts(),
			"sender": event.sender(),
			"state_key": event.state_key(),
			"type": event.kind(),
		});

		if let Some(unsigned) = event.unsigned() {
			json["unsigned"] = json!(unsigned);
		}

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}

impl<E: Event> From<Owned<E>> for Raw<AnyStrippedStateEvent> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<AnyStrippedStateEvent> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let json = json!({
			"content": event.content(),
			"sender": event.sender(),
			"state_key": event.state_key(),
			"type": event.kind(),
		});

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}

impl<E: Event> From<Owned<E>> for Raw<HierarchySpaceChildEvent> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<HierarchySpaceChildEvent> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let json = json!({
			"content": event.content(),
			"origin_server_ts": event.origin_server_ts(),
			"sender": event.sender(),
			"state_key": event.state_key(),
			"type": event.kind(),
		});

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}

impl<E: Event> From<Owned<E>> for Raw<StateEvent<RoomMemberEventContent>> {
	fn from(event: Owned<E>) -> Self { Ref(&event.0).into() }
}

impl<'a, E: Event> From<Ref<'a, E>> for Raw<StateEvent<RoomMemberEventContent>> {
	fn from(event: Ref<'a, E>) -> Self {
		let event = event.0;
		let mut json = json!({
			"content": event.content(),
			"event_id": event.event_id(),
			"origin_server_ts": event.origin_server_ts(),
			"redacts": event.redacts(),
			"room_id": event.room_id(),
			"sender": event.sender(),
			"state_key": event.state_key(),
			"type": event.kind(),
		});

		if let Some(unsigned) = event.unsigned() {
			json["unsigned"] = json!(unsigned);
		}

		serde_json::from_value(json).expect("Failed to serialize Event value")
	}
}
