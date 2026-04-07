use std::str::FromStr;

use conduwuit::{
	Err, Result, err, implement,
	matrix::event::{gen_event_id, gen_event_id_canonical_json},
};
use itertools::Itertools;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, OwnedRoomId, RoomId, RoomVersionId };
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
	if event_type != "m.room.create" || pdu.get("state_key").is_none() {
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

	if !room_version_rules.authorization.room_create_event_id_as_room_id {
		return Err!(Request(BadJson("Missing room_id in pdu")));
	}

	let event_id = gen_event_id(pdu, &room_version_rules)?;
	Ok(RoomId::parse(event_id.as_str().replace('$', "!"))
		.expect("constructed room ID has to be valid"))
}

/// Parses every entry in an array as an event ID, returning an error if any
/// step fails.
fn expect_event_id_array(value: &CanonicalJsonObject, field: &str) -> Result<Vec<OwnedEventId>> {
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

/// Performs some basic validation on the PDU to make sure it's not obviously
/// malformed. This is not a full validation, but guards against extreme errors.
///
/// Currently, this just validates that prev/auth events are within acceptable
/// ranges. Other servers do some additional things like checking depth range,
/// but serde will do that later when converting the object to a PduEvent.
#[implement(super::Service)]
pub fn validate_pdu(&self, pdu: &CanonicalJsonObject) -> Result {
	// Since v3:
	// `event_id` should not be present on the PDU.
	// NOTE: The above is ignored since technically it's still allowed to be
	// included, but should be ignored instead.
	// `auth_events` and `prev_events` must be an array of event IDs
	let auth_events = expect_event_id_array(pdu, "auth_events")?;
	if auth_events.len() > 10 {
		return Err!(Request(BadJson("PDU has too many auth events")));
	}
	let prev_events = expect_event_id_array(pdu, "prev_events")?;
	if prev_events.len() > 20 {
		return Err!(Request(BadJson("PDU has too many prev events")));
	}
	Ok(())
}

#[implement(super::Service)]
pub async fn parse_incoming_pdu(&self, pdu: &RawJsonValue) -> Result<Parsed> {
	let value = serde_json::from_str::<CanonicalJsonObject>(pdu.get()).map_err(|e| {
		err!(BadServerResponse(debug_warn!("Error parsing incoming event {e:?}")))
	})?;
	let event_type = value
		.get("type")
		.and_then(CanonicalJsonValue::as_str)
		.ok_or_else(|| err!(Request(InvalidParam("Missing or invalid type in pdu"))))?;

	let room_id = extract_room_id(event_type, &value)?;

	let room_version_rules = self
		.services
		.state
		.get_room_version(&room_id)
		.await
		.unwrap_or(RoomVersionId::V1)
		.rules()
		.unwrap();

	let (event_id, value) = gen_event_id_canonical_json(pdu, &room_version_rules).map_err(|e| {
		err!(Request(InvalidParam("Could not convert event to canonical json: {e}")))
	})?;
	self.validate_pdu(&value)?;
	Ok((room_id, event_id, value))
}
