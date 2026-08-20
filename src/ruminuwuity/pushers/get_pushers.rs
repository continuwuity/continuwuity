pub mod v3 {
	use ruma::{
		api::{auth_scheme::AccessToken, error::Error as MatrixError, request, response},
		metadata,
		serde::from_raw_json_value,
	};
	use serde::{Deserialize, Serialize, de};
	use serde_json::value::RawValue as RawJsonValue;

	use crate::pushers::Pusher;

	metadata! {
		method: GET,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_matrix/client/r0/pushers",
			1.1 => "/_matrix/client/v3/pushers",
		}
	}

	#[request(error = MatrixError)]
	#[derive(Default)]
	#[allow(clippy::empty_structs_with_brackets, reason = "required by #[request]")]
	pub struct Request {}

	#[response(error = MatrixError)]
	pub struct Response {
		pub pushers: Vec<PusherWithActivation>,
	}

	#[derive(Clone, Debug, Serialize)]
	pub struct PusherWithActivation {
		#[serde(flatten)]
		pub pusher: Pusher,

		#[serde(skip_serializing_if = "Option::is_none")]
		pub activated: Option<bool>,
	}

	#[derive(Deserialize)]
	struct ActivationDeHelper {
		activated: Option<bool>,
	}

	impl<'de> Deserialize<'de> for PusherWithActivation {
		fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where
			D: de::Deserializer<'de>,
		{
			let json = Box::<RawJsonValue>::deserialize(deserializer)?;

			let ActivationDeHelper { activated } = from_raw_json_value(&json)?;
			let pusher = from_raw_json_value(&json)?;

			Ok(Self { pusher, activated })
		}
	}

	impl Request {
		#[must_use]
		pub fn new() -> Self { Self {} }
	}

	impl Response {
		#[must_use]
		pub fn new(pushers: Vec<PusherWithActivation>) -> Self { Self { pushers } }
	}
}
