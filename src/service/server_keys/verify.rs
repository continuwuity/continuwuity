use conduwuit::{Err, Result, implement, matrix::event::gen_event_id_canonical_json};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, RoomVersionId, signatures::Verified,
};
use serde_json::value::RawValue as RawJsonValue;

#[implement(super::Service)]
pub async fn validate_and_add_event_id(
	&self,
	pdu: &RawJsonValue,
	room_version: &RoomVersionId,
) -> Result<(OwnedEventId, CanonicalJsonObject)> {
	let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version)?;
	if let Err(e) = self.verify_event(&value, Some(room_version)).await {
		return Err!(BadServerResponse(debug_error!(
			"Event {event_id} failed verification: {e:?}"
		)));
	}

	value.insert("event_id".into(), CanonicalJsonValue::String(event_id.as_str().into()));

	Ok((event_id, value))
}

#[implement(super::Service)]
pub async fn validate_and_add_event_id_no_fetch(
	&self,
	pdu: &RawJsonValue,
	room_version: &RoomVersionId,
) -> Result<(OwnedEventId, CanonicalJsonObject)> {
	let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version)?;
	if !self.required_keys_exist(&value, room_version).await {
		return Err!(BadServerResponse(debug_warn!(
			"Event {event_id} cannot be verified: missing keys."
		)));
	}

	if let Err(e) = self.verify_event(&value, Some(room_version)).await {
		return Err!(BadServerResponse(debug_error!(
			"Event {event_id} failed verification: {e:?}"
		)));
	}

	value.insert("event_id".into(), CanonicalJsonValue::String(event_id.as_str().into()));

	Ok((event_id, value))
}

#[implement(super::Service)]
pub async fn verify_event(
	&self,
	event: &CanonicalJsonObject,
	room_version: Option<&RoomVersionId>,
) -> Result<Verified> {
	let room_version = room_version.unwrap_or(&RoomVersionId::V11);
	let keys = self.get_event_keys(event, room_version).await?;
	ruma::signatures::verify_event(&keys, event, room_version).map_err(Into::into)
}

#[implement(super::Service)]
pub async fn verify_json(
	&self,
	event: &CanonicalJsonObject,
	room_version: Option<&RoomVersionId>,
) -> Result {
	let room_version = room_version.unwrap_or(&RoomVersionId::V11);
	let keys = self.get_event_keys(event, room_version).await?;
	ruma::signatures::verify_json(&keys, event.clone()).map_err(Into::into)
}
