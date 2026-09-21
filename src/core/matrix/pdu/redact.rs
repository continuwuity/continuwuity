use ruma::{RoomVersionId, canonical_json::redact_content_in_place};
use serde_json::{json, value::to_raw_value};

use crate::{Err, Result, err};

impl super::Pdu {
	pub fn redact(
		&mut self,
		room_version_id: &RoomVersionId,
		redacted_because: Option<&ruma::EventId>,
	) -> Result {
		let Some(rules) = room_version_id.rules() else {
			return Err!("Cannot redact event for unknown room version {room_version_id}");
		};

		self.unsigned = None;

		// a redacted sticky event is just a normal event (MSC4354)
		self.sticky = None;

		let mut content = serde_json::from_str(self.content.get()).map_err(|e| {
			err!(Request(BadJson("Failed to deserialize content into type: {e}")))
		})?;

		redact_content_in_place(&mut content, &rules.redaction, self.kind.to_string());

		if let Some(event_id) = redacted_because {
			self.unsigned = to_raw_value(&json!({
				"org.continuwuity.redacted_by": event_id,
			}))
			.expect("Failed to serialize unsigned")
			.into();
		}

		self.content = to_raw_value(&content).expect("Failed to serialize content");

		Ok(())
	}
}
