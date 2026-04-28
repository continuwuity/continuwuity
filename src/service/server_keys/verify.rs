use conduwuit::{
	Err, Result, debug_warn, implement, matrix::event::gen_event_id_canonical_json, trace,
};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, room_version_rules::RoomVersionRules,
	signatures::Verified,
};
use serde_json::value::RawValue as RawJsonValue;

#[implement(super::Service)]
pub async fn validate_and_add_event_id(
	&self,
	pdu: &RawJsonValue,
	room_version_rules: &RoomVersionRules,
) -> Result<(OwnedEventId, CanonicalJsonObject)> {
	let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version_rules)?;
	if let Err(e) = self.verify_event(&value, room_version_rules).await {
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
	room_version_rules: &RoomVersionRules,
) -> Result<(OwnedEventId, CanonicalJsonObject)> {
	trace!(?pdu, "Validating PDU without fetching keys");
	let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version_rules)?;
	trace!(event_id = event_id.as_str(), "Generated event ID, checking required keys");
	if !self.required_keys_exist(&value, room_version_rules).await {
		debug_warn!(
			"Event {event_id} is missing required keys, cannot verify without fetching keys"
		);
		return Err!(BadServerResponse(debug_warn!(
			"Event {event_id} cannot be verified: missing keys."
		)));
	}
	trace!("All required keys exist, verifying event");
	if let Err(e) = self.verify_event(&value, room_version_rules).await {
		debug_warn!("Event verification failed");
		return Err!(BadServerResponse(debug_error!(
			"Event {event_id} failed verification: {e:?}"
		)));
	}
	trace!("Event verified successfully");

	value.insert("event_id".into(), CanonicalJsonValue::String(event_id.as_str().into()));

	Ok((event_id, value))
}

#[implement(super::Service)]
pub async fn verify_event(
	&self,
	event: &CanonicalJsonObject,
	room_version_rules: &RoomVersionRules,
) -> Result<Verified> {
	let keys = self.get_event_keys(event, room_version_rules).await?;
	ruma::signatures::verify_event(&keys, event, room_version_rules).map_err(Into::into)
}

#[implement(super::Service)]
pub async fn verify_json(
	&self,
	event: &CanonicalJsonObject,
	room_version_rules: &RoomVersionRules,
) -> Result {
	let keys = self.get_event_keys(event, room_version_rules).await?;
	ruma::signatures::verify_json(&keys, event).map_err(Into::into)
}
