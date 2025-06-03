use conduwuit::{Event, PduEvent, Result, err};
use ruma::{
	EventId, RoomId, UserId,
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
		room_id: &RoomId,
		event_id: &EventId,
	) -> Result<Option<BundledMessageLikeRelations<Box<serde_json::value::RawValue>>>> {
		let relations = self
			.get_relations(
				user_id,
				room_id,
				event_id,
				conduwuit::PduCount::max(),
				MAX_BUNDLED_RELATIONS,
				0,
				Direction::Backward,
			)
			.await;
		// The relations database code still handles the basic unsigned data
		// We don't want to recursively fetch relations

		// TODO: Event visibility check
		// TODO: ignored users?

		if relations.is_empty() {
			return Ok(None);
		}

		let mut replace_events = Vec::with_capacity(relations.len().min(10)); // Most events have few replacements
		let mut reference_events = Vec::with_capacity(relations.len());

		for relation in &relations {
			let pdu = &relation.1;

			let content = pdu.get_content_as_value();
			if let Some(relates_to) = content.get("m.relates_to") {
				// We don't check that the event relates back, because we assume the database is
				// good.
				if let Some(rel_type) = relates_to.get("rel_type") {
					match rel_type.as_str() {
						| Some("m.replace") => {
							replace_events.push(relation);
						},
						| Some("m.reference") => {
							reference_events.push(relation);
						},
						| _ => {
							// Ignore other relation types for now
							// Threads are in the database but not handled here
							// Other types are not specified AFAICT.
						},
					}
				}
			}
		}

		// If no relations to bundle, return None
		if replace_events.is_empty() && reference_events.is_empty() {
			return Ok(None);
		}

		let mut bundled = BundledMessageLikeRelations::new();

		// Handle m.replace relations - find the most recent one
		if !replace_events.is_empty() {
			let most_recent_replacement = Self::find_most_recent_replacement(&replace_events)?;

			// Convert the replacement event to the bundled format
			if let Some(replacement_pdu) = most_recent_replacement {
				// According to the Matrix spec, we should include the full event as raw JSON
				let replacement_json = serde_json::to_string(replacement_pdu)
					.map_err(|e| err!(Database("Failed to serialize replacement event: {e}")))?;
				let raw_value = serde_json::value::RawValue::from_string(replacement_json)
					.map_err(|e| err!(Database("Failed to create RawValue: {e}")))?;
				bundled.replace = Some(Box::new(raw_value));
			}
		}

		// Handle m.reference relations - collect event IDs
		if !reference_events.is_empty() {
			let reference_chunk = Self::build_reference_chunk(&reference_events)?;
			if !reference_chunk.is_empty() {
				bundled.reference = Some(Box::new(ReferenceChunk::new(reference_chunk)));
			}
		}

		// TODO: Handle other relation types (m.annotation, etc.) when specified

		Ok(Some(bundled))
	}

	/// Build reference chunk for m.reference bundled aggregations
	fn build_reference_chunk(
		reference_events: &[&PdusIterItem],
	) -> Result<Vec<BundledReference>> {
		let mut chunk = Vec::with_capacity(reference_events.len());

		for relation in reference_events {
			let pdu = &relation.1;

			let reference_entry = BundledReference::new(pdu.event_id().to_owned());
			chunk.push(reference_entry);
		}

		// Don't sort, order is unspecified

		Ok(chunk)
	}

	/// Find the most recent replacement event based on origin_server_ts and
	/// lexicographic event_id ordering
	fn find_most_recent_replacement<'a>(
		replacement_events: &'a [&'a PdusIterItem],
	) -> Result<Option<&'a PduEvent>> {
		if replacement_events.is_empty() {
			return Ok(None);
		}

		let mut most_recent: Option<&PduEvent> = None;

		// Jank, is there a better way to do this?
		for relation in replacement_events {
			let pdu = &relation.1;

			match most_recent {
				| None => {
					most_recent = Some(pdu);
				},
				| Some(current_most_recent) => {
					// Compare by origin_server_ts first
					match pdu
						.origin_server_ts()
						.cmp(&current_most_recent.origin_server_ts())
					{
						| std::cmp::Ordering::Greater => {
							most_recent = Some(pdu);
						},
						| std::cmp::Ordering::Equal => {
							// If timestamps are equal, use lexicographic ordering of event_id
							if pdu.event_id() > current_most_recent.event_id() {
								most_recent = Some(pdu);
							}
						},
						| std::cmp::Ordering::Less => {
							// Keep current most recent
						},
					}
				},
			}
		}

		Ok(most_recent)
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

		let bundled_aggregations = self
			.get_bundled_aggregations(user_id, &pdu.room_id_or_hash(), pdu.event_id())
			.await?;

		if let Some(aggregations) = bundled_aggregations {
			let aggregations_json = serde_json::to_value(aggregations)
				.map_err(|e| err!(Database("Failed to serialize bundled aggregations: {e}")))?;

			Self::add_bundled_aggregations_to_unsigned(pdu, aggregations_json)?;
		}

		Ok(())
	}

	/// Helper method to add bundled aggregations to a PDU's unsigned
	/// field
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
			for (rel_type, aggregation) in aggregations_map {
				relations.insert(rel_type, aggregation);
			}
		}

		pdu.unsigned = Some(to_raw_value(&unsigned)?);

		Ok(())
	}
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
}
