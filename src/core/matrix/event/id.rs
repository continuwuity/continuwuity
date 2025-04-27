use ruma::{CanonicalJsonObject, OwnedEventId, RoomVersionId};
use serde_json::value::RawValue as RawJsonValue;

use crate::{Result, err};

/// Generates a correct eventId for the incoming pdu.
///
/// Returns a tuple of the new `EventId` and the PDU as a `BTreeMap<String,
/// CanonicalJsonValue>`.
pub fn gen_event_id_canonical_json(
	pdu: &RawJsonValue,
	room_version_id: &RoomVersionId,
) -> Result<(OwnedEventId, CanonicalJsonObject)> {
	let value: CanonicalJsonObject = serde_json::from_str(pdu.get())
		.map_err(|e| err!(BadServerResponse(warn!("Error parsing incoming event: {e:?}"))))?;

	let event_id = gen_event_id(&value, room_version_id)?;

	Ok((event_id, value))
}

/// Generates a correct eventId for the incoming pdu.
pub fn gen_event_id(
	value: &CanonicalJsonObject,
	room_version_id: &RoomVersionId,
) -> Result<OwnedEventId> {
	let reference_hash = ruma::signatures::reference_hash(value, room_version_id)?;
	let event_id: OwnedEventId = format!("${reference_hash}").try_into()?;

	Ok(event_id)
}
