use ruma::{
	CanonicalJsonObject, EventId, OwnedEventId,
	room_version_rules::{EventIdFormatVersion, RoomVersionRules},
};
use serde_json::value::RawValue as RawJsonValue;

use crate::{Result, err};

/// Generates a correct eventId for the incoming pdu.
///
/// Returns a tuple of the new `EventId` and the PDU as a `BTreeMap<String,
/// CanonicalJsonValue>`.
pub fn gen_event_id_canonical_json(
	pdu: &RawJsonValue,
	room_version_rules: &RoomVersionRules,
) -> Result<(OwnedEventId, CanonicalJsonObject)> {
	let value: CanonicalJsonObject = serde_json::from_str(pdu.get())
		.map_err(|e| err!(BadServerResponse(warn!("Error parsing incoming event: {e:?}"))))?;

	let event_id = gen_event_id(&value, room_version_rules)?;

	Ok((event_id, value))
}

/// Generates a correct eventId for the incoming pdu.
pub fn gen_event_id(
	value: &CanonicalJsonObject,
	room_version_rules: &RoomVersionRules,
) -> Result<OwnedEventId> {
	assert_ne!(
		room_version_rules.event_id_format,
		EventIdFormatVersion::V1,
		"Continuwuity does not support PDU v1"
	);
	let reference_hash = ruma::signatures::reference_hash(value, room_version_rules)?;
	Ok(EventId::new_v2_or_v3(&reference_hash)?)
}
