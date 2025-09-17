use conduwuit::{
	Result, RoomVersion, err, implement, matrix::event::gen_event_id_canonical_json,
	result::FlatOk,
};
use ruma::{CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, OwnedRoomId, RoomVersionId};
use serde_json::value::RawValue as RawJsonValue;

type Parsed = (OwnedRoomId, OwnedEventId, CanonicalJsonObject);

#[implement(super::Service)]
pub async fn parse_incoming_pdu(&self, pdu: &RawJsonValue) -> Result<Parsed> {
	let value = serde_json::from_str::<CanonicalJsonObject>(pdu.get()).map_err(|e| {
		err!(BadServerResponse(debug_warn!("Error parsing incoming event {e:?}")))
	})?;
	let event_type = value
		.get("type")
		.and_then(CanonicalJsonValue::as_str)
		.ok_or_else(|| err!(Request(InvalidParam("Missing or invalid type in pdu"))))?;

	let room_id: OwnedRoomId = if event_type != "m.room.create" {
		value
			.get("room_id")
			.and_then(CanonicalJsonValue::as_str)
			.map(OwnedRoomId::parse)
			.flat_ok_or(err!(Request(InvalidParam("Invalid room_id in pdu"))))?
	} else {
		// v12 rooms might have no room_id in the create event. We'll need to check the
		// content.room_version
		let content = value
			.get("content")
			.and_then(CanonicalJsonValue::as_object)
			.ok_or_else(|| err!(Request(InvalidParam("Missing or invalid content in pdu"))))?;
		let room_version = content
			.get("room_version")
			.and_then(CanonicalJsonValue::as_str)
			.unwrap_or("1");
		let vi = RoomVersionId::try_from(room_version).unwrap_or(RoomVersionId::V1);
		let vf = RoomVersion::new(&vi).expect("supported room version");
		if vf.room_ids_as_hashes {
			let (event_id, _) = gen_event_id_canonical_json(pdu, &vi).map_err(|e| {
				err!(Request(InvalidParam("Could not convert event to canonical json: {e}")))
			})?;
			OwnedRoomId::parse(event_id.as_str().replace('$', "!")).expect("valid room ID")
		} else {
			// V11 or below room, room_id must be present
			value
				.get("room_id")
				.and_then(CanonicalJsonValue::as_str)
				.map(OwnedRoomId::parse)
				.flat_ok_or(err!(Request(InvalidParam("Invalid or missing room_id in pdu"))))?
		}
	};

	let room_version_id = self
		.services
		.state
		.get_room_version(&room_id)
		.await
		.map_err(|_| err!("Server is not in room {room_id}"))?;
	let (event_id, value) = gen_event_id_canonical_json(pdu, &room_version_id).map_err(|e| {
		err!(Request(InvalidParam("Could not convert event to canonical json: {e}")))
	})?;
	Ok((room_id, event_id, value))
}
