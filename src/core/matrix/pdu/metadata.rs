use ruma::{EventId, OwnedEventId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PduMetadata {
	/// This event's calculated event ID.
	pub event_id: OwnedEventId,

	/// Indicates whether the event has been rejected under auth rules.
	pub rejected: bool,
}

impl PduMetadata {
	/// Creates a new PduMetadata instance with the minimum required fields.
	pub fn new(event_id: OwnedEventId) -> Self { Self { event_id, rejected: false } }
}
