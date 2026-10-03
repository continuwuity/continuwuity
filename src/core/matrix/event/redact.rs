use ruma::{
	OwnedEventId,
	events::{TimelineEventType, room::redaction::RoomRedactionEventContent},
	room_version_rules::RoomVersionRules,
};
use serde::Deserialize;
use serde_json::value::{RawValue as RawJsonValue, to_raw_value};

use super::Event;

/// Copies the `redacts` property of the event to the `content` dict and
/// vice-versa.
///
/// This follows the specification's
/// [recommendation](https://spec.matrix.org/v1.10/rooms/v11/#moving-the-redacts-property-of-mroomredaction-events-to-a-content-property):
///
/// > For backwards-compatibility with older clients, servers should add a
/// > redacts property to the top level of m.room.redaction events in when
/// > serving such events over the Client-Server API.
///
/// > For improved compatibility with newer clients, servers should add a
/// > redacts property to the content of m.room.redaction events in older
/// > room versions when serving such events over the Client-Server API.
#[must_use]
pub(super) fn copy<E: Event>(event: &E) -> (Option<OwnedEventId>, Box<RawJsonValue>) {
	if *event.event_type() != TimelineEventType::RoomRedaction {
		return (event.redacts().map(ToOwned::to_owned), event.content().to_owned());
	}

	let Ok(mut content) = event.get_content::<RoomRedactionEventContent>() else {
		return (event.redacts().map(ToOwned::to_owned), event.content().to_owned());
	};

	if let Some(redacts) = content.redacts {
		return (Some(redacts), event.content().to_owned());
	}

	if let Some(redacts) = event.redacts().map(ToOwned::to_owned) {
		content.redacts = Some(redacts);
		return (
			event.redacts().map(ToOwned::to_owned),
			to_raw_value(&content).expect("Must be valid, we only added redacts field"),
		);
	}

	(event.redacts().map(ToOwned::to_owned), event.content().to_owned())
}

#[must_use]
pub(super) fn is_redacted<E: Event>(event: &E) -> bool {
	let Some(unsigned) = event.unsigned() else {
		return false;
	};

	let Ok(unsigned) = ExtractRedactedBecause::deserialize(unsigned) else {
		return false;
	};

	unsigned.redacted_by.is_some()
}

#[derive(Deserialize)]
struct RedactionField {
	redacts: OwnedEventId,
}

#[must_use]
pub(super) fn redacts_id<E: Event>(
	event: &E,
	room_version_rules: &RoomVersionRules,
) -> Option<OwnedEventId> {
	if *event.kind() != TimelineEventType::RoomRedaction {
		return None;
	}

	if room_version_rules.redaction.content_field_redacts {
		event
			.get_content::<RedactionField>()
			.map(|r| r.redacts)
			.ok()
	} else {
		event.redacts().map(ToOwned::to_owned)
	}
}

#[derive(Deserialize)]
struct ExtractRedactedBecause {
	#[serde(rename = "org.continuwuity.redacted_by")]
	redacted_by: Option<serde::de::IgnoredAny>,
}

#[cfg(test)]
mod tests {
	use ruma::{UInt, events::TimelineEventType, room_version_rules::RoomVersionRules};
	use serde_json::json;

	use crate::{Event, Pdu, pdu::EventHash};

	#[test]
	fn v11_redacts_from_content() {
		let p = Pdu {
			event_id: ruma::owned_event_id!("$v11"),
			room_id: Some(ruma::owned_room_id!("!v11:example.com")),
			sender: ruma::owned_user_id!("@user:example.com"),
			origin: None,
			origin_server_ts: UInt::default(),
			kind: TimelineEventType::RoomRedaction,
			content: serde_json::value::to_raw_value(&json!({"redacts": "$content_redacts"}))
				.unwrap(),
			sticky: None,
			state_key: None,
			prev_events: vec![],
			depth: UInt::default(),
			auth_events: vec![],
			redacts: Some(ruma::owned_event_id!("$apex_redacts")),
			unsigned: None,
			hashes: EventHash { sha256: String::new() },
			signatures: None,
		};
		assert_eq!(
			p.redacts_id(&RoomVersionRules::V11),
			Some(ruma::owned_event_id!("$content_redacts"))
		);
	}

	#[test]
	fn v10_redacts_from_apex() {
		let p = Pdu {
			event_id: ruma::owned_event_id!("$v11"),
			room_id: Some(ruma::owned_room_id!("!v11:example.com")),
			sender: ruma::owned_user_id!("@user:example.com"),
			origin: None,
			origin_server_ts: UInt::default(),
			kind: TimelineEventType::RoomRedaction,
			content: serde_json::value::to_raw_value(&json!({"redacts": "$content_redacts"}))
				.unwrap(),
			sticky: None,
			state_key: None,
			prev_events: vec![],
			depth: UInt::default(),
			auth_events: vec![],
			redacts: Some(ruma::owned_event_id!("$apex_redacts")),
			unsigned: None,
			hashes: EventHash { sha256: String::new() },
			signatures: None,
		};
		assert_eq!(
			p.redacts_id(&RoomVersionRules::V10),
			Some(ruma::owned_event_id!("$apex_redacts"))
		);
	}

	#[test]
	fn redacts_ignores_non_redaction_event() {
		let p = Pdu {
			event_id: ruma::owned_event_id!("$v11"),
			room_id: Some(ruma::owned_room_id!("!v11:example.com")),
			sender: ruma::owned_user_id!("@user:example.com"),
			origin: None,
			origin_server_ts: UInt::default(),
			kind: TimelineEventType::RoomMessage,
			content: serde_json::value::to_raw_value(&json!({"redacts": "$content_redacts"}))
				.unwrap(),
			sticky: None,
			state_key: None,
			prev_events: vec![],
			depth: UInt::default(),
			auth_events: vec![],
			redacts: Some(ruma::owned_event_id!("$apex_redacts")),
			unsigned: None,
			hashes: EventHash { sha256: String::new() },
			signatures: None,
		};
		assert_eq!(p.redacts_id(&RoomVersionRules::V11), None);
		assert_eq!(p.redacts_id(&RoomVersionRules::V10), None);
	}
}
