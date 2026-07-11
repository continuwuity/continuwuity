use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// The content hash map for a PDU.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hashes {
	/// The SHA256 content hash.
	pub sha256: String,

	/// Any other hashes present, if any.
	#[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
	pub other: HashMap<String, String>,
}
