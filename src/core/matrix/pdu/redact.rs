use ruma::{RoomVersionId, canonical_json::redact_content_in_place};
use serde_json::{Value as JsonValue, json, value::to_raw_value};

use crate::{Error, Result, err, implement};

#[implement(super::Pdu)]
pub fn redact(&mut self, room_version_id: &RoomVersionId, reason: JsonValue) -> Result {
	self.unsigned = None;

	let mut content = serde_json::from_str(self.content.get())
		.map_err(|e| err!(Request(BadJson("Failed to deserialize content into type: {e}"))))?;

	redact_content_in_place(&mut content, room_version_id, self.kind.to_string())
		.map_err(|e| Error::Redaction(self.sender.server_name().to_owned(), e))?;

	let reason = serde_json::to_value(reason).expect("Failed to preserialize reason");

	let redacted_because = json!({
		"redacted_because": reason,
	});

	self.unsigned = to_raw_value(&redacted_because)
		.expect("Failed to serialize unsigned")
		.into();

	self.content = to_raw_value(&content).expect("Failed to serialize content");

	Ok(())
}
