pub mod ack;
pub mod get_pushers;
mod pusher_serde;
pub mod set_pusher;

use std::fmt;

pub use ruma::api::client::push::{EmailPusherData, PusherIds};
use ruma::{
	push::{HttpPusherData, PushFormat},
	serde::JsonObject,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize)]
pub struct Pusher {
	#[serde(flatten)]
	pub ids: PusherIds,

	#[serde(flatten)]
	pub kind: PusherKind,

	pub app_display_name: String,

	pub device_display_name: String,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub profile_tag: Option<String>,

	pub lang: String,
}

#[derive(Clone, Debug)]
pub enum PusherKind {
	Http(HttpPusherData),
	Email(EmailPusherData),
	WebPush(WebPushPusherData),

	Custom {
		kind: String,
		data: JsonObject,
	},
}

#[derive(Clone, Serialize, Deserialize)]
pub struct WebPushPusherData {
	pub url: String,

	pub auth: String,

	#[serde(skip_serializing_if = "Option::is_none")]
	pub format: Option<PushFormat>,

	#[serde(flatten, default, skip_serializing_if = "JsonObject::is_empty")]
	pub data: JsonObject,
}

impl fmt::Debug for WebPushPusherData {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("WebPushPusherData")
			.field("url", &self.url)
			.field("auth", &"<redacted>")
			.field("format", &self.format)
			.field("data", &self.data)
			.finish()
	}
}

impl PusherKind {
	#[must_use]
	pub fn as_str(&self) -> &str {
		match self {
			| Self::Http(_) => "http",
			| Self::Email(_) => "email",
			| Self::WebPush(_) => "org.matrix.msc4174.webpush",
			| Self::Custom { kind, .. } => kind,
		}
	}

	#[must_use]
	pub fn as_webpush(&self) -> Option<&WebPushPusherData> {
		match self {
			| Self::WebPush(data) => Some(data),
			| _ => None,
		}
	}
}
