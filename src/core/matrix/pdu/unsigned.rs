use std::{borrow::Borrow, collections::BTreeMap};

use ruma::MilliSecondsSinceUnixEpoch;
use serde_json::value::{RawValue as RawJsonValue, Value as JsonValue, to_raw_value};

use super::Pdu;
use crate::{Result, err, implement, result::LogErr};

/// Set the `unsigned` field of the PDU using only information in the PDU.
/// Some unsigned data is already set within the database (eg. prev events,
/// threads). Once this is done, other data must be calculated from the database
/// (eg. relations) This is for server-to-client events.
/// Backfill handles this itself.
#[implement(Pdu)]
pub fn set_unsigned(&mut self, user_id: Option<&ruma::UserId>) {
	if Some(self.sender.borrow()) != user_id {
		self.remove_transaction_id().log_err().ok();
	}
	self.add_age().log_err().ok();
}

#[implement(Pdu)]
pub fn remove_transaction_id(&mut self) -> Result {
	use BTreeMap as Map;

	let Some(unsigned) = &self.unsigned else {
		return Ok(());
	};

	let mut unsigned: Map<&str, Box<RawJsonValue>> = serde_json::from_str(unsigned.get())
		.map_err(|e| err!(Database("Invalid unsigned in pdu event: {e}")))?;

	unsigned.remove("transaction_id");
	self.unsigned = to_raw_value(&unsigned)
		.map(Some)
		.expect("unsigned is valid");

	Ok(())
}

#[implement(Pdu)]
pub fn add_age(&mut self) -> Result {
	use BTreeMap as Map;

	let mut unsigned: Map<&str, Box<RawJsonValue>> = self
		.unsigned
		.as_deref()
		.map(RawJsonValue::get)
		.map_or_else(|| Ok(Map::new()), serde_json::from_str)
		.map_err(|e| err!(Database("Invalid unsigned in pdu event: {e}")))?;

	// deliberately allowing for the possibility of negative age
	let now: i128 = MilliSecondsSinceUnixEpoch::now().get().into();
	let then: i128 = self.origin_server_ts.into();
	let this_age = now.saturating_sub(then);

	unsigned.insert("age", to_raw_value(&this_age)?);
	self.unsigned = Some(to_raw_value(&unsigned)?);

	Ok(())
}

#[implement(Pdu)]
pub fn add_relation(&mut self, name: &str, pdu: Option<&Pdu>) -> Result {
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
