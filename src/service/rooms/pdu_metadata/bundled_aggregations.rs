use conduwuit::{Event, PduEvent, Result, err};
use ruma::{
	UserId,
	api::Direction,
	events::relation::{BundledMessageLikeRelations, BundledReference, ReferenceChunk},
};

use crate::rooms::timeline::PdusIterItem;

const MAX_BUNDLED_RELATIONS: usize = 50;

impl super::Service {
	/// Gets bundled aggregations for an event according to the Matrix
	/// specification.
	/// - m.replace relations are bundled to include the most recent replacement
	///   event.
	/// - m.reference relations are bundled to include a chunk of event IDs.
	#[tracing::instrument(skip(self), level = "debug")]
	pub async fn get_bundled_aggregations(
		&self,
		user_id: &UserId,
		pdu: &PduEvent,
	) -> Result<Option<BundledMessageLikeRelations<Box<serde_json::value::RawValue>>>> {
		// Events that can never get bundled aggregations
		if pdu.state_key().is_some() || Self::is_replacement_event(pdu) {
			return Ok(None);
		}

		let relations = self
			.get_relations(
				user_id,
				&pdu.room_id_or_hash(),
				pdu.event_id(),
				conduwuit::PduCount::max(),
				MAX_BUNDLED_RELATIONS,
				0,
				Direction::Backward,
			)
			.await;

		// The relations database code still handles the basic unsigned data
		// We don't want to recursively fetch relations

		if relations.is_empty() {
			return Ok(None);
		}

		// Partition relations by type
		let (replace_events, reference_events): (Vec<_>, Vec<_>) = relations
			.iter()
			.filter_map(|relation| {
				let pdu = &relation.1;
				let content = pdu.get_content_as_value();

				content
					.get("m.relates_to")
					.and_then(|relates_to| relates_to.get("rel_type"))
					.and_then(|rel_type| rel_type.as_str())
					.and_then(|rel_type_str| match rel_type_str {
						| "m.replace" => Some(RelationType::Replace(relation)),
						| "m.reference" => Some(RelationType::Reference(relation)),
						| _ => None, /* Ignore other relation types (threads are in DB but not
						              * handled here) */
					})
			})
			.fold((Vec::new(), Vec::new()), |(mut replaces, mut references), rel_type| {
				match rel_type {
					| RelationType::Replace(r) => replaces.push(r),
					| RelationType::Reference(r) => references.push(r),
				}
				(replaces, references)
			});

		// If no relations to bundle, return None
		if replace_events.is_empty() && reference_events.is_empty() {
			return Ok(None);
		}

		let mut bundled = BundledMessageLikeRelations::<Box<serde_json::value::RawValue>>::new();

		// Handle m.replace relations - find the most recent valid one (lazy load
		// original event)
		if !replace_events.is_empty() {
			if let Some(replacement) = Self::find_most_recent_valid_replacement(
				pdu,
				&replace_events,
				async |pdu: &PduEvent| {
					self.services
						.state_accessor
						.user_can_see_event(user_id, &pdu.room_id_or_hash(), &pdu.event_id())
						.await
				},
			)
			.await?
			{
				bundled.replace = Some(Self::serialize_replacement(replacement)?);
			}
		}

		// Handle m.reference relations - collect event IDs
		if !reference_events.is_empty() {
			let reference_chunk: Vec<_> = reference_events
				.into_iter()
				.map(|relation| BundledReference::new(relation.1.event_id().to_owned()))
				.collect();

			if !reference_chunk.is_empty() {
				bundled.reference = Some(Box::new(ReferenceChunk::new(reference_chunk)));
			}
		}

		// TODO: Handle other relation types (m.annotation, etc.) when specified

		Ok(Some(bundled))
	}

	/// Serialize a replacement event to the bundled format
	fn serialize_replacement(pdu: &PduEvent) -> Result<Box<Box<serde_json::value::RawValue>>> {
		let replacement_json = serde_json::to_string(pdu)
			.map_err(|e| err!(Database("Failed to serialize replacement event: {e}")))?;

		let raw_value = serde_json::value::RawValue::from_string(replacement_json)
			.map_err(|e| err!(Database("Failed to create RawValue: {e}")))?;

		Ok(Box::new(raw_value))
	}

	/// Find the most recent valid replacement event based on origin_server_ts
	/// and lexicographic event_id ordering
	async fn find_most_recent_valid_replacement<'a>(
		original_event: &PduEvent,
		replacement_events: &[&'a PdusIterItem],
		visible: impl AsyncFn(&PduEvent) -> bool,
	) -> Result<Option<&'a PduEvent>> {
		// Filter valid replacements and find the maximum in a single pass
		let mut result: Option<&PduEvent> = None;

		for relation in replacement_events {
			let pdu = &relation.1;

			// Validate replacement
			if !Self::is_valid_replacement_event(original_event, pdu).await? {
				continue;
			}

			let next = match result {
				| None => Some(pdu),
				| Some(current) => {
					// Compare by origin_server_ts first, then event_id lexicographically
					match pdu.origin_server_ts().cmp(&current.origin_server_ts()) {
						| std::cmp::Ordering::Greater => Some(pdu),
						| std::cmp::Ordering::Equal if pdu.event_id() > current.event_id() =>
							Some(pdu),
						| _ => None,
					}
				},
			};
			if let Some(pdu) = next
				&& visible(pdu).await
			{
				result = Some(pdu);
			}
		}

		Ok(result)
	}

	/// Adds bundled aggregations to a PDU's unsigned field
	#[tracing::instrument(skip(self, pdu), level = "debug")]
	pub async fn add_bundled_aggregations_to_pdu(
		&self,
		user_id: &UserId,
		pdu: &mut PduEvent,
	) -> Result<()> {
		if pdu.is_redacted() {
			return Ok(());
		}

		let bundled_aggregations = self.get_bundled_aggregations(user_id, pdu).await?;

		if let Some(aggregations) = bundled_aggregations {
			let aggregations_json = serde_json::to_value(aggregations)
				.map_err(|e| err!(Database("Failed to serialize bundled aggregations: {e}")))?;

			Self::add_bundled_aggregations_to_unsigned(pdu, aggregations_json)?;
		}

		Ok(())
	}

	/// Helper method to add bundled aggregations to a PDU's unsigned field
	fn add_bundled_aggregations_to_unsigned(
		pdu: &mut PduEvent,
		aggregations_json: serde_json::Value,
	) -> Result<()> {
		use serde_json::{
			Map, Value as JsonValue,
			value::{RawValue as RawJsonValue, to_raw_value},
		};

		let mut unsigned: Map<String, JsonValue> = pdu
			.unsigned
			.as_deref()
			.map(RawJsonValue::get)
			.map_or_else(|| Ok(Map::new()), serde_json::from_str)
			.map_err(|e| err!(Database("Invalid unsigned in pdu event: {e}")))?;

		let relations = unsigned
			.entry("m.relations")
			.or_insert_with(|| JsonValue::Object(Map::new()))
			.as_object_mut()
			.ok_or_else(|| err!(Database("m.relations is not an object")))?;

		if let JsonValue::Object(aggregations_map) = aggregations_json {
			relations.extend(aggregations_map);
		}

		pdu.unsigned = Some(to_raw_value(&unsigned)?);

		Ok(())
	}

	/// Validates that an event is acceptable as a replacement for another event
	/// See C/S spec "Validity of replacement events"
	#[tracing::instrument(level = "debug")]
	async fn is_valid_replacement_event(
		original_event: &PduEvent,
		replacement_event: &PduEvent,
	) -> Result<bool> {
		Ok(
			// 1. Same room_id
			original_event.room_id() == replacement_event.room_id()
			// 2. Same sender
			&& original_event.sender() == replacement_event.sender()
			// 3. Same type
			&& original_event.event_type() == replacement_event.event_type()
			// 4. Neither event should have a state_key property
			&& original_event.state_key().is_none()
			&& replacement_event.state_key().is_none()
			// 5. Original event must not have rel_type of m.replace
			&& !Self::is_replacement_event(original_event)
			// 6. Replacement event must have m.new_content property (skip for encrypted)
			&& Self::has_new_content_or_encrypted(replacement_event),
		)
	}

	/// Check if an event is itself a replacement
	#[inline]
	fn is_replacement_event(event: &PduEvent) -> bool {
		event
			.get_content_as_value()
			.get("m.relates_to")
			.and_then(|relates_to| relates_to.get("rel_type"))
			.and_then(|rel_type| rel_type.as_str())
			.is_some_and(|rel_type| rel_type == "m.replace")
	}

	/// Check if event has m.new_content or is encrypted (where m.new_content
	/// would be in the encrypted payload)
	#[inline]
	fn has_new_content_or_encrypted(event: &PduEvent) -> bool {
		event.event_type() == &ruma::events::TimelineEventType::RoomEncrypted
			|| event.get_content_as_value().get("m.new_content").is_some()
	}
}

/// Helper enum for partitioning relations
enum RelationType<'a> {
	Replace(&'a PdusIterItem),
	Reference(&'a PdusIterItem),
}

#[cfg(test)]
mod tests {
	use conduwuit_core::pdu::{EventHash, PduEvent};
	use ruma::{UInt, events::TimelineEventType, owned_event_id, owned_room_id, owned_user_id};
	use serde_json::{Value as JsonValue, json, value::to_raw_value};

	fn create_test_pdu(unsigned_content: Option<JsonValue>) -> PduEvent {
		PduEvent {
			event_id: owned_event_id!("$test:example.com"),
			room_id: Some(owned_room_id!("!test:example.com")),
			sender: owned_user_id!("@test:example.com"),
			origin_server_ts: UInt::try_from(1_234_567_890_u64).unwrap(),
			kind: TimelineEventType::RoomMessage,
			content: to_raw_value(&json!({"msgtype": "m.text", "body": "test"})).unwrap(),
			state_key: None,
			prev_events: vec![],
			depth: UInt::from(1_u32),
			auth_events: vec![],
			redacts: None,
			unsigned: unsigned_content.map(|content| to_raw_value(&content).unwrap()),
			hashes: EventHash { sha256: "test_hash".to_owned() },
			signatures: None,
			origin: None,
		}
	}

	fn create_bundled_aggregations() -> JsonValue {
		json!({
			"m.replace": {
				"event_id": "$replace:example.com",
				"origin_server_ts": 1_234_567_890,
				"sender": "@replacer:example.com"
			},
			"m.reference": {
				"count": 5,
				"chunk": [
					"$ref1:example.com",
					"$ref2:example.com"
				]
			}
		})
	}

	#[test]
	fn test_add_bundled_aggregations_to_unsigned_no_existing_unsigned() {
		let mut pdu = create_test_pdu(None);
		let aggregations = create_bundled_aggregations();

		let result = super::super::Service::add_bundled_aggregations_to_unsigned(
			&mut pdu,
			aggregations.clone(),
		);
		assert!(result.is_ok(), "Should succeed when no unsigned field exists");

		assert!(pdu.unsigned.is_some(), "Unsigned field should be created");

		let unsigned_str = pdu.unsigned.as_ref().unwrap().get();
		let unsigned: JsonValue = serde_json::from_str(unsigned_str).unwrap();

		assert!(unsigned.get("m.relations").is_some(), "m.relations should exist");
		assert_eq!(
			unsigned["m.relations"], aggregations,
			"Relations should match the aggregations"
		);
	}

	#[test]
	fn test_add_bundled_aggregations_to_unsigned_overwrite_same_relation_type() {
		let existing_unsigned = json!({
			"m.relations": {
				"m.replace": {
					"event_id": "$old_replace:example.com",
					"origin_server_ts": 1_111_111_111,
					"sender": "@old_replacer:example.com"
				}
			}
		});

		let mut pdu = create_test_pdu(Some(existing_unsigned));
		let new_aggregations = create_bundled_aggregations();

		let result = super::super::Service::add_bundled_aggregations_to_unsigned(
			&mut pdu,
			new_aggregations.clone(),
		);
		assert!(result.is_ok(), "Should succeed when overwriting same relation type");

		let unsigned_str = pdu.unsigned.as_ref().unwrap().get();
		let unsigned: JsonValue = serde_json::from_str(unsigned_str).unwrap();

		let relations = &unsigned["m.relations"];

		assert_eq!(
			relations["m.replace"], new_aggregations["m.replace"],
			"m.replace should be updated"
		);
		assert_eq!(
			relations["m.replace"]["event_id"], "$replace:example.com",
			"Should have new event_id"
		);

		assert!(relations.get("m.reference").is_some(), "New m.reference should be added");
	}

	#[test]
	fn test_add_bundled_aggregations_to_unsigned_preserve_other_unsigned_fields() {
		// Test case: Other unsigned fields should be preserved
		let existing_unsigned = json!({
			"age": 98765,
			"prev_content": {"msgtype": "m.text", "body": "old message"},
			"redacted_because": {"event_id": "$redaction:example.com"},
			"m.relations": {
				"m.annotation": {"count": 1}
			}
		});

		let mut pdu = create_test_pdu(Some(existing_unsigned));
		let new_aggregations = json!({
			"m.replace": {"event_id": "$new:example.com"}
		});

		let result = super::super::Service::add_bundled_aggregations_to_unsigned(
			&mut pdu,
			new_aggregations,
		);
		assert!(result.is_ok(), "Should succeed while preserving other fields");

		let unsigned_str = pdu.unsigned.as_ref().unwrap().get();
		let unsigned: JsonValue = serde_json::from_str(unsigned_str).unwrap();

		// Verify all existing fields are preserved
		assert_eq!(unsigned["age"], 98765, "age should be preserved");
		assert!(unsigned.get("prev_content").is_some(), "prev_content should be preserved");
		assert!(
			unsigned.get("redacted_because").is_some(),
			"redacted_because should be preserved"
		);

		// Verify relations were merged correctly
		let relations = &unsigned["m.relations"];
		assert!(
			relations.get("m.annotation").is_some(),
			"Existing m.annotation should be preserved"
		);
		assert!(relations.get("m.replace").is_some(), "New m.replace should be added");
	}

	#[test]
	fn test_add_bundled_aggregations_to_unsigned_invalid_existing_unsigned() {
		// Test case: Invalid JSON in existing unsigned should result in error
		let mut pdu = create_test_pdu(None);
		// Manually set invalid unsigned data
		pdu.unsigned = Some(to_raw_value(&"invalid json").unwrap());

		let aggregations = create_bundled_aggregations();
		let result =
			super::super::Service::add_bundled_aggregations_to_unsigned(&mut pdu, aggregations);

		assert!(result.is_err(), "fails when existing unsigned is invalid");
		// Should we ignore the error and overwrite anyway?
	}

	// Test helper function to create test PDU events
	fn create_test_event(
		event_id: &str,
		room_id: &str,
		sender: &str,
		event_type: TimelineEventType,
		content: &JsonValue,
		state_key: Option<&str>,
	) -> PduEvent {
		PduEvent {
			event_id: event_id.try_into().unwrap(),
			room_id: Some(room_id.try_into().unwrap()),
			sender: sender.try_into().unwrap(),
			origin_server_ts: UInt::try_from(1_234_567_890_u64).unwrap(),
			kind: event_type,
			content: to_raw_value(&content).unwrap(),
			state_key: state_key.map(Into::into),
			prev_events: vec![],
			depth: UInt::from(1_u32),
			auth_events: vec![],
			redacts: None,
			unsigned: None,
			hashes: EventHash { sha256: "test_hash".to_owned() },
			signatures: None,
			origin: None,
		}
	}

	/// Test that a valid replacement event passes validation
	#[tokio::test]
	async fn test_valid_replacement_event() {
		let original = create_test_event(
			"$original:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({"msgtype": "m.text", "body": "original message"}),
			None,
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({
				"msgtype": "m.text",
				"body": "* edited message",
				"m.new_content": {
					"msgtype": "m.text",
					"body": "edited message"
				},
				"m.relates_to": {
					"rel_type": "m.replace",
					"event_id": "$original:example.com"
				}
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(result.unwrap(), "Valid replacement event should be accepted");
	}

	/// Test replacement event with different room ID is rejected
	#[tokio::test]
	async fn test_replacement_event_different_room() {
		let original = create_test_event(
			"$original:example.com",
			"!room1:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({"msgtype": "m.text", "body": "original message"}),
			None,
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room2:example.com", // Different room
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({
				"msgtype": "m.text",
				"body": "* edited message",
				"m.new_content": {
					"msgtype": "m.text",
					"body": "edited message"
				}
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(!result.unwrap(), "Different room ID should be rejected");
	}

	/// Test replacement event with different sender is rejected
	#[tokio::test]
	async fn test_replacement_event_different_sender() {
		let original = create_test_event(
			"$original:example.com",
			"!room:example.com",
			"@user1:example.com",
			TimelineEventType::RoomMessage,
			&json!({"msgtype": "m.text", "body": "original message"}),
			None,
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room:example.com",
			"@user2:example.com", // Different sender
			TimelineEventType::RoomMessage,
			&json!({
				"msgtype": "m.text",
				"body": "* edited message",
				"m.new_content": {
					"msgtype": "m.text",
					"body": "edited message"
				}
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(!result.unwrap(), "Different sender should be rejected");
	}

	/// Test replacement event with different type is rejected
	#[tokio::test]
	async fn test_replacement_event_different_type() {
		let original = create_test_event(
			"$original:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({"msgtype": "m.text", "body": "original message"}),
			None,
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomTopic, // Different event type
			&json!({
				"topic": "new topic",
				"m.new_content": {
					"topic": "new topic"
				}
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(!result.unwrap(), "Different event type should be rejected");
	}

	/// Test replacement event with state key is rejected
	#[tokio::test]
	async fn test_replacement_event_with_state_key() {
		let original = create_test_event(
			"$original:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomName,
			&json!({"name": "room name"}),
			Some(""), // Has state key
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomName,
			&json!({
				"name": "new room name",
				"m.new_content": {
					"name": "new room name"
				}
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(!result.unwrap(), "Event with state key should be rejected");
	}

	/// Test replacement of an event that is already a replacement is rejected
	#[tokio::test]
	async fn test_replacement_event_original_is_replacement() {
		let original = create_test_event(
			"$original:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({
				"msgtype": "m.text",
				"body": "* edited message",
				"m.relates_to": {
					"rel_type": "m.replace", // Original is already a replacement
					"event_id": "$some_other:example.com"
				}
			}),
			None,
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({
				"msgtype": "m.text",
				"body": "* edited again",
				"m.new_content": {
					"msgtype": "m.text",
					"body": "edited again"
				}
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(!result.unwrap(), "Replacement of replacement should be rejected");
	}

	/// Test replacement event missing m.new_content is rejected
	#[tokio::test]
	async fn test_replacement_event_missing_new_content() {
		let original = create_test_event(
			"$original:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({"msgtype": "m.text", "body": "original message"}),
			None,
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomMessage,
			&json!({
				"msgtype": "m.text",
				"body": "* edited message"
				// Missing m.new_content
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(!result.unwrap(), "Missing m.new_content should be rejected");
	}

	/// Test encrypted replacement event without m.new_content is accepted
	#[tokio::test]
	async fn test_replacement_event_encrypted_missing_new_content_is_valid() {
		let original = create_test_event(
			"$original:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomEncrypted,
			&json!({
				"algorithm": "m.megolm.v1.aes-sha2",
				"ciphertext": "encrypted_payload_base64",
				"sender_key": "sender_key",
				"session_id": "session_id"
			}),
			None,
		);

		let replacement = create_test_event(
			"$replacement:example.com",
			"!room:example.com",
			"@user:example.com",
			TimelineEventType::RoomEncrypted,
			&json!({
				"algorithm": "m.megolm.v1.aes-sha2",
				"ciphertext": "encrypted_replacement_payload_base64",
				"sender_key": "sender_key",
				"session_id": "session_id",
				"m.relates_to": {
					"rel_type": "m.replace",
					"event_id": "$original:example.com"
				}
				// No m.new_content in cleartext - this is valid for encrypted events
			}),
			None,
		);

		let result =
			super::super::Service::is_valid_replacement_event(&original, &replacement).await;
		assert!(result.is_ok(), "Validation should succeed");
		assert!(
			result.unwrap(),
			"Encrypted replacement without cleartext m.new_content should be accepted"
		);
	}
}
