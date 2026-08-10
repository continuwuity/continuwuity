use ruma::serde::from_raw_json_value;
use serde::{Deserialize, Serialize, de, ser::SerializeStruct};
use serde_json::value::RawValue as RawJsonValue;

use super::{Pusher, PusherIds, PusherKind};

#[derive(Debug, Deserialize)]
struct PusherDeHelper {
	#[serde(flatten)]
	ids: PusherIds,
	app_display_name: String,
	device_display_name: String,
	profile_tag: Option<String>,
	lang: String,
}

impl<'de> Deserialize<'de> for Pusher {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: de::Deserializer<'de>,
	{
		let json = Box::<RawJsonValue>::deserialize(deserializer)?;

		let PusherDeHelper {
			ids,
			app_display_name,
			device_display_name,
			profile_tag,
			lang,
		} = from_raw_json_value(&json)?;
		let kind = from_raw_json_value(&json)?;

		Ok(Self {
			ids,
			kind,
			app_display_name,
			device_display_name,
			profile_tag,
			lang,
		})
	}
}

impl Serialize for PusherKind {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: serde::Serializer,
	{
		let mut st = serializer.serialize_struct("PusherKind", 2)?;
		st.serialize_field("kind", self.as_str())?;

		match self {
			| Self::Http(data) => st.serialize_field("data", data)?,
			| Self::Email(data) => st.serialize_field("data", data)?,
			| Self::WebPush(data) => st.serialize_field("data", data)?,
			| Self::Custom { data, .. } => st.serialize_field("data", data)?,
		}

		st.end()
	}
}

#[derive(Debug, Deserialize)]
struct PusherKindDeHelper {
	kind: String,
	data: Box<RawJsonValue>,
}

impl<'de> Deserialize<'de> for PusherKind {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: de::Deserializer<'de>,
	{
		let json = Box::<RawJsonValue>::deserialize(deserializer)?;
		let PusherKindDeHelper { kind, data } = from_raw_json_value(&json)?;

		match kind.as_str() {
			| "http" => from_raw_json_value(&data).map(Self::Http),
			| "email" => from_raw_json_value(&data).map(Self::Email),
			| "org.matrix.msc4174.webpush" => from_raw_json_value(&data).map(Self::WebPush),
			| _ => Ok(Self::Custom { kind, data: from_raw_json_value(&data)? }),
		}
	}
}

#[cfg(test)]
mod tests {
	use serde_json::{from_value as from_json_value, json, to_value as to_json_value};

	use super::{Pusher, PusherKind};

	#[test]
	fn roundtrip_ruma_http_pusher() {
		let stored = json!({
			"pushkey": "abcdef",
			"app_id": "my.matrix.app",
			"kind": "http",
			"data": {
				"url": "https://push.example.org/_matrix/push/v1/notify",
				"format": "event_id_only",
				"default_payload": { "session_id": "abc" },
			},
			"app_display_name": "My Matrix App",
			"device_display_name": "My Phone",
			"profile_tag": "tag",
			"lang": "en",
		});

		let pusher: Pusher = from_json_value(stored.clone()).unwrap();
		assert!(matches!(pusher.kind, PusherKind::Http(_)));
		assert_eq!(to_json_value(&pusher).unwrap(), stored);
	}

	#[test]
	fn deserialize_legacy_append_field() {
		let stored = json!({
			"pushkey": "abcdef",
			"app_id": "my.matrix.app",
			"kind": "email",
			"data": {},
			"append": true,
			"app_display_name": "My Matrix App",
			"device_display_name": "My Phone",
			"lang": "en",
		});

		let pusher: Pusher = from_json_value(stored).unwrap();
		assert!(matches!(pusher.kind, PusherKind::Email(_)));
		assert_eq!(pusher.profile_tag, None);
	}

	#[test]
	fn roundtrip_webpush_pusher() {
		let stored = json!({
			"pushkey": "BLn9b-VR0ca83knDNZ32dCHGyjJp",
			"app_id": "m.webpush",
			"kind": "org.matrix.msc4174.webpush",
			"data": {
				"url": "https://updates.push.services.mozilla.com/wpush/v2/abc",
				"auth": "_ordMnz7uTCmrpBTeUV4Bw",
				"format": "event_id_only",
				"default_payload": { "session_id": "abc" },
			},
			"app_display_name": "WebPush",
			"device_display_name": "Firefox",
			"lang": "en",
		});

		let pusher: Pusher = from_json_value(stored.clone()).unwrap();
		let data = pusher.kind.as_webpush().unwrap();
		assert_eq!(data.auth, "_ordMnz7uTCmrpBTeUV4Bw");
		assert_eq!(data.data.len(), 1);
		assert_eq!(to_json_value(&pusher).unwrap(), stored);
	}

	#[test]
	fn roundtrip_pusher_with_activation() {
		use crate::pushers::get_pushers::v3::PusherWithActivation;

		let wire = json!({
			"pushkey": "BLn9b-VR0ca83knDNZ32dCHGyjJp",
			"app_id": "m.webpush",
			"kind": "org.matrix.msc4174.webpush",
			"data": {
				"url": "https://updates.push.services.mozilla.com/wpush/v2/abc",
				"auth": "_ordMnz7uTCmrpBTeUV4Bw",
			},
			"app_display_name": "WebPush",
			"device_display_name": "Firefox",
			"lang": "en",
			"activated": true,
		});

		let pusher: PusherWithActivation = from_json_value(wire.clone()).unwrap();
		assert_eq!(pusher.activated, Some(true));
		assert_eq!(pusher.pusher.kind.as_str(), "org.matrix.msc4174.webpush");
		assert_eq!(to_json_value(&pusher).unwrap(), wire);
	}

	#[test]
	fn roundtrip_unknown_kind() {
		let stored = json!({
			"pushkey": "abcdef",
			"app_id": "my.matrix.app",
			"kind": "my.custom.kind",
			"data": { "whatever": 1 },
			"app_display_name": "App",
			"device_display_name": "Device",
			"lang": "en",
		});

		let pusher: Pusher = from_json_value(stored.clone()).unwrap();
		assert_eq!(pusher.kind.as_str(), "my.custom.kind");
		assert_eq!(to_json_value(&pusher).unwrap(), stored);
	}
}
