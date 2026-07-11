use std::str::FromStr;

use conduwuit::{
	Err, Result, err,
	matrix::event::{gen_event_id, gen_event_id_canonical_json},
};
use itertools::Itertools;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, OwnedRoomId, RoomId,
	RoomVersionId, room_version_rules::RoomVersionRules,
};
use serde_json::value::RawValue as RawJsonValue;

type Parsed = (OwnedRoomId, OwnedEventId, CanonicalJsonObject);

/// Extracts the expected room ID from the PDU. If the PDU claims its own room
/// ID, that is returned. Since `m.room.create` in v12 and onward lacks this
/// field over federation, it will be calculated if not provided, otherwise a
/// validation error will be returned.
fn extract_room_id(event_type: &str, pdu: &CanonicalJsonObject) -> Result<OwnedRoomId> {
	if let Some(room_id) = pdu.get("room_id").and_then(CanonicalJsonValue::as_str) {
		return RoomId::parse(room_id)
			.map_err(|e| err!(Request(BadJson("Invalid room_id {room_id:?} in pdu: {e}"))));
	}
	// If there's no room ID, and this is not a create event, it is illegal.
	let is_create = event_type == "m.room.create"
		&& pdu
			.get("state_key")
			.and_then(|v| v.as_str())
			.is_some_and(str::is_empty);
	if !is_create {
		return Err!(Request(BadJson("Missing room_id in pdu")));
	}

	// Room versions 11 and below require the room ID is present.
	let room_version = RoomVersionId::from_str(
		pdu.get("content")
			.and_then(CanonicalJsonValue::as_object)
			.ok_or_else(|| err!(Request(InvalidParam("Missing or invalid content in pdu"))))?
			.get("room_version")
			.and_then(CanonicalJsonValue::as_str)
			.unwrap_or("1"), // Omitted room versions default to v1
	)
	.map_err(|e| err!(Request(BadJson("Invalid room_version in pdu: {e}"))))?;

	let Some(room_version_rules) = room_version.rules() else {
		return Err!(Request(BadJson("Unknown room version in pdu")));
	};

	if room_version_rules.event_format.require_room_create_room_id {
		return Err!(Request(BadJson("Missing room_id in pdu")));
	}

	let event_id = gen_event_id(pdu, &room_version_rules)?;
	Ok(RoomId::parse(event_id.as_str().replace('$', "!"))
		.expect("constructed room ID has to be valid"))
}

/// Parses every entry in an array as an event ID, returning an error if any
/// step fails.
pub(super) fn expect_event_id_array(
	value: &CanonicalJsonObject,
	field: &str,
) -> Result<Vec<OwnedEventId>> {
	value
		.get(field)
		.ok_or_else(|| err!(Request(BadJson("missing field `{field}` on PDU"))))?
		.as_array()
		.ok_or_else(|| err!(Request(BadJson("expected an array PDU field `{field}`"))))?
		.iter()
		.map(|v| {
			v.as_str()
				.ok_or_else(|| {
					err!(Request(BadJson("expected an array of event IDs for `{field}`")))
				})
				.and_then(|s| {
					EventId::parse(s)
						.map_err(|e| err!(Request(BadJson("invalid event ID in `{field}`: {e}"))))
				})
		})
		.try_collect()
}

impl super::Service {
	/// Parses an incoming PDU JSON object, generating an event ID for it and
	/// attempts to discover the associated room ID. Does not insert the event
	/// ID into the returned object.
	pub async fn parse_incoming_pdu(
		&self,
		pdu: &RawJsonValue,
		room_version_rules: Option<&RoomVersionRules>,
	) -> Result<Parsed> {
		let value = serde_json::from_str::<CanonicalJsonObject>(pdu.get()).map_err(|e| {
			err!(BadServerResponse(debug_warn!("Error parsing incoming event {e:?}")))
		})?;
		let event_type = value
			.get("type")
			.and_then(CanonicalJsonValue::as_str)
			.ok_or_else(|| err!(Request(InvalidParam("Missing or invalid type in pdu"))))?;

		let room_id = extract_room_id(event_type, &value)?;

		let room_version_rules = match room_version_rules {
			| Some(r) => r,
			| None => &self
				.services
				.state
				.get_room_version(&room_id)
				.await
				.unwrap_or(RoomVersionId::V1)
				.rules()
				.expect("room version must be supported"),
		};

		let (event_id, value) =
			gen_event_id_canonical_json(pdu, room_version_rules).map_err(|e| {
				err!(Request(InvalidParam("Could not convert event to canonical json: {e}")))
			})?;
		// NOTE: validation checks are now performed by `pdu_format_check_1`.
		Ok((room_id, event_id, value))
	}
}
