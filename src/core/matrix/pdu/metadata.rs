use ruma::OwnedEventId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PduMetadata {
	/// This event's calculated event ID.
	pub event_id: OwnedEventId,

	/// Indicates whether the event has been rejected under auth rules.
	pub rejected: bool,
}
