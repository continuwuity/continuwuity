//! Web Push key validation, token comparison, and payload shaping helpers.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use conduwuit_core::{Err, Result, err};
use ruma::{api::push_gateway::send_event_notification::v1::Notification, serde::JsonObject};
use serde_json::Value as JsonValue;
use web_push_native::{Auth, p256::PublicKey};

const MAX_PAYLOAD: usize = 3900;
const MAX_BODY_LENGTH: usize = 1000;
const MAX_CIPHERTEXT_LENGTH: usize = 2000;

pub(crate) fn decode_pushkey(pushkey: &str) -> Result<PublicKey> {
	let bytes = URL_SAFE_NO_PAD
		.decode(pushkey)
		.map_err(|e| err!(Request(InvalidParam("Push key is not URL-safe base64: {e}"))))?;
	if bytes.len() != 65 || bytes.first() != Some(&0x04) {
		return Err!(Request(InvalidParam(
			"Push key must be a P-256 public key in the uncompressed form"
		)));
	}

	PublicKey::from_sec1_bytes(&bytes)
		.map_err(|e| err!(Request(InvalidParam("Push key is not a valid P-256 public key: {e}"))))
}

pub(crate) fn decode_auth(auth: &str) -> Result<Auth> {
	let bytes = URL_SAFE_NO_PAD
		.decode(auth)
		.map_err(|e| err!(Request(InvalidParam("Auth secret is not URL-safe base64: {e}"))))?;
	let bytes: [u8; 16] = bytes
		.try_into()
		.map_err(|_| err!(Request(InvalidParam("Auth secret must be 16 bytes"))))?;

	Ok(Auth::from(bytes))
}

pub(super) fn tokens_match(a: &str, b: &str) -> bool {
	a.len() == b.len()
		&& a.bytes()
			.zip(b.bytes())
			.fold(0_u8, |acc, (x, y)| acc | (x ^ y))
			== 0
}

pub(super) fn build_payload(notify: &Notification, data: &JsonObject) -> Result<Vec<u8>> {
	let JsonValue::Object(mut payload) = serde_json::to_value(notify)? else {
		return Err!("Notification did not serialize to an object");
	};
	payload.remove("devices");

	if let Some(JsonValue::Object(counts)) = payload.remove("counts") {
		for key in ["unread", "missed_calls"] {
			if let Some(count) = counts.get(key) {
				payload.insert(key.to_owned(), count.clone());
			}
		}
	}
	if let Some(JsonValue::Object(default_payload)) = data.get("default_payload") {
		for (key, value) in default_payload {
			payload.insert(key.clone(), value.clone());
		}
	}
	if let Some(JsonValue::Object(content)) = payload.get_mut("content") {
		if let Some(JsonValue::String(body)) = content.get("body")
			&& body.len() > MAX_BODY_LENGTH
		{
			content.insert(
				"body".to_owned(),
				format!("{}…", truncate_bytes(body, MAX_BODY_LENGTH)).into(),
			);
		}
		if content
			.get("ciphertext")
			.and_then(JsonValue::as_str)
			.is_some_and(|ciphertext| ciphertext.len() > MAX_CIPHERTEXT_LENGTH)
		{
			content.remove("ciphertext");
		}
	}

	let mut encoded = serde_json::to_vec(&payload)?;
	if encoded.len() > MAX_PAYLOAD {
		payload.remove("content");
		encoded = serde_json::to_vec(&payload)?;
	}
	if encoded.len() > MAX_PAYLOAD {
		return Err!(Request(TooLarge("Web push payload is too large to deliver")));
	}

	Ok(encoded)
}

fn truncate_bytes(text: &str, max: usize) -> String {
	let mut truncated = String::with_capacity(max);
	for ch in text.chars() {
		if truncated.len().saturating_add(ch.len_utf8()) > max {
			break;
		}
		truncated.push(ch);
	}

	truncated
}

#[cfg(test)]
mod tests {
	use ruma::{
		api::push_gateway::send_event_notification::v1::{Notification, NotificationCounts},
		push::PushFormat,
		serde::JsonObject,
		uint,
	};
	use serde_json::{Value as JsonValue, json};

	use super::{MAX_BODY_LENGTH, build_payload, tokens_match};

	fn payload_of(notify: &Notification, data: &JsonObject) -> JsonObject {
		let encoded = build_payload(notify, data).unwrap();
		match serde_json::from_slice(&encoded).unwrap() {
			| JsonValue::Object(payload) => payload,
			| other => panic!("expected an object, got {other}"),
		}
	}

	fn notification_with_body(body: &str) -> Notification {
		let mut notify = Notification::new(vec![]);
		notify.content = Some(serde_json::value::to_raw_value(&json!({ "body": body })).unwrap());

		notify
	}

	#[test]
	fn flattens_counts_and_drops_devices() {
		let mut notify = Notification::new(vec![]);
		notify.counts = NotificationCounts::new(uint!(3), uint!(1));
		let payload = payload_of(&notify, &JsonObject::new());

		assert!(!payload.contains_key("devices"));
		assert!(!payload.contains_key("counts"));
		assert_eq!(payload.get("unread"), Some(&json!(3)));
		assert_eq!(payload.get("missed_calls"), Some(&json!(1)));
	}

	#[test]
	fn merges_default_payload() {
		let mut data = JsonObject::new();
		data.insert("default_payload".to_owned(), json!({ "session_id": "abc" }));
		data.insert("format".to_owned(), json!(PushFormat::EventIdOnly));
		let payload = payload_of(&Notification::new(vec![]), &data);

		assert_eq!(payload.get("session_id"), Some(&json!("abc")));
		assert!(!payload.contains_key("format"));
	}

	#[test]
	fn truncates_long_body() {
		let payload = payload_of(&notification_with_body(&"a".repeat(2000)), &JsonObject::new());
		let body = payload["content"]["body"].as_str().unwrap();

		assert!(body.len() <= MAX_BODY_LENGTH + '…'.len_utf8());
		assert!(body.ends_with('…'));
	}

	#[test]
	fn truncates_multibyte_body_by_bytes() {
		let payload = payload_of(&notification_with_body(&"é".repeat(2000)), &JsonObject::new());
		let body = payload["content"]["body"].as_str().unwrap();

		assert!(body.len() <= MAX_BODY_LENGTH + '…'.len_utf8());
		assert!(body.chars().count() < 2000);
	}

	#[test]
	fn rejects_payload_that_cannot_be_shrunk() {
		let mut data = JsonObject::new();
		data.insert("default_payload".to_owned(), json!({ "pad": "x".repeat(4000) }));

		build_payload(&Notification::new(vec![]), &data).unwrap_err();
	}

	#[test]
	fn token_comparison() {
		assert!(tokens_match("abc", "abc"));
		assert!(!tokens_match("abc", "abz"));
		assert!(!tokens_match("abc", "abcd"));
	}
}
