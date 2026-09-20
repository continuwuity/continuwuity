use std::collections::BTreeMap;

use ruma::{
	MilliSecondsSinceUnixEpoch,
	events::{AnyTimelineEvent, room::member::MembershipState},
	serde::Raw,
};
use serde_json::value::{RawValue as RawJsonValue, Value as JsonValue, to_raw_value};

use super::{Pdu, sticky};
use crate::{Event, Result, err, result::LogErr};

type Unsigned = BTreeMap<String, Box<RawJsonValue>>;

impl Pdu {
	/// Set the `unsigned` field of the PDU using only information in the PDU.
	/// Some unsigned data is already set within the database (eg. prev events,
	/// threads). Once this is done, other data must be calculated from the
	/// database (eg. relations) This is for server-to-client events.
	/// Backfill handles this itself.
	pub fn set_unsigned(
		&mut self,
		user_id: Option<&ruma::UserId>,
		membership: Option<MembershipState>,
		prev_content: Option<Box<RawJsonValue>>,
		redacted_because: Option<Raw<AnyTimelineEvent>>,
	) {
		// See: https://spec.matrix.org/v1.19/client-server-api/#definition-clientevent_unsigneddata
		let Some(mut unsigned) = self
			.unsigned()
			.and_then(|u| serde_json::from_str::<Unsigned>(u.get()).ok())
		else {
			return;
		};

		let now: ruma::Int = MilliSecondsSinceUnixEpoch::now().get().into();
		unsigned.insert(
			"age".to_owned(),
			to_raw_value(&now.saturating_sub(self.origin_server_ts.into())).unwrap(),
		);

		if let Some(membership) = membership {
			unsigned.insert("membership".to_owned(), to_raw_value(&membership).unwrap());
		}

		if let Some(prev_content) = prev_content {
			unsigned.insert(
				"prev_content".to_owned(),
				to_raw_value(&prev_content)
					.expect("prev_content must be a valid JSON value for unsigned"),
			);
		}
		if let Some(redacted_because) = redacted_because {
			unsigned.remove("redacted_because_id");
			unsigned.insert(
				"redacted_because".to_owned(),
				to_raw_value(&redacted_because)
					.expect("redacted_because must be a valid JSON value for unsigned"),
			);
		}

		// Remove transaction_id unless the user is the sender
		if user_id.is_none_or(|u| u != self.sender()) {
			unsigned.remove("transaction_id");
		}
		self.add_sticky_duration_ttl().log_err().ok();
	}

	pub fn add_sticky_duration_ttl(&mut self) -> Result {
		use BTreeMap as Map;

		let now = u64::from(MilliSecondsSinceUnixEpoch::now().get());
		let Some(expires_at) = self
			.sticky
			.as_deref()
			.and_then(|sticky| sticky::expires_at(self.origin_server_ts, sticky, now))
		else {
			return Ok(());
		};

		let mut unsigned: Map<&str, Box<RawJsonValue>> = self
			.unsigned
			.as_deref()
			.map(RawJsonValue::get)
			.map_or_else(|| Ok(Map::new()), serde_json::from_str)
			.map_err(|e| err!(Database("Invalid unsigned in pdu event: {e}")))?;

		unsigned.insert(sticky::TTL_UNSIGNED_KEY, to_raw_value(&expires_at.saturating_sub(now))?);
		self.unsigned = Some(to_raw_value(&unsigned)?);

		Ok(())
	}

	pub fn add_relation(&mut self, name: &str, pdu: Option<&Self>) -> Result {
		use serde_json::Map;

		let mut unsigned: Map<String, JsonValue> = self
			.unsigned
			.as_deref()
			.map(RawJsonValue::get)
			.map_or_else(|| Ok(Map::new()), serde_json::from_str)
			.map_err(|e| err!(Database("Invalid unsigned in pdu event: {e}")))?;

		let pdu = pdu
			.map(serde_json::to_value)
			.transpose()?
			.unwrap_or_else(|| JsonValue::Object(Map::new()));

		unsigned
			.entry("m.relations")
			.or_insert(JsonValue::Object(Map::new()))
			.as_object_mut()
			.map(|object| object.insert(name.to_owned(), pdu));

		self.unsigned = Some(to_raw_value(&unsigned)?);

		Ok(())
	}
}
