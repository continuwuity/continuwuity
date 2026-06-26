use conduwuit::{Err, Result, matrix::event::gen_event_id_canonical_json};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, room_version_rules::RoomVersionRules,
	signatures::Verified,
};
use serde_json::value::RawValue as RawJsonValue;

impl super::Service {
	/// Validates the incoming event, and then inserts the calculated event ID
	/// into the event_id field.
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

	/// Validates the incoming event only using locally known verification keys.
	/// Inserts the calculated event ID into the event_id field upon success.
	pub async fn validate_and_add_event_id_no_fetch(
		&self,
		pdu: &RawJsonValue,
		room_version_rules: &RoomVersionRules,
	) -> Result<(OwnedEventId, CanonicalJsonObject)> {
		let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version_rules)?;
		if !self.required_keys_exist(&value, room_version_rules).await {
			return Err!(BadServerResponse(debug_warn!(
				"Event {event_id} cannot be verified: missing keys."
			)));
		}
		if let Err(e) = self.verify_event(&value, room_version_rules).await {
			return Err!(BadServerResponse(debug_error!(
				"Event {event_id} failed verification: {e:?}"
			)));
		}

		value.insert("event_id".into(), CanonicalJsonValue::String(event_id.as_str().into()));

		Ok((event_id, value))
	}

	/// Verifies the incoming event after fetching the keys required to do so.
	pub async fn verify_event(
		&self,
		event: &CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result<Verified> {
		let keys = self.get_event_keys(event, room_version_rules).await?;
		ruma::signatures::verify_event(&keys, event, room_version_rules).map_err(Into::into)
	}

	/// Verifies an arbitrary JSON object after fetching the keys required to do
	/// so.
	pub async fn verify_json(
		&self,
		event: &CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result {
		// TODO: is this actually used anywhere?
		let keys = self.get_event_keys(event, room_version_rules).await?;
		ruma::signatures::verify_json(&keys, event).map_err(Into::into)
	}
}
