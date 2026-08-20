pub mod v3 {
	use bytes::BufMut;
	use ruma::{
		JsOption,
		api::{
			IncomingResponse, OutgoingResponse,
			auth_scheme::AccessToken,
			error::{DeserializationError, Error as MatrixError, IntoHttpError},
			request,
		},
		metadata,
		serde::from_raw_json_value,
	};
	use serde::{Deserialize, Serialize, de};
	use serde_json::value::RawValue as RawJsonValue;

	use crate::pushers::{Pusher, PusherIds};

	metadata! {
		method: POST,
		rate_limited: true,
		authentication: AccessToken,
		history: {
			1.0 => "/_matrix/client/r0/pushers/set",
			1.1 => "/_matrix/client/v3/pushers/set",
		}
	}

	#[request(error = MatrixError)]
	pub struct Request {
		#[ruma_api(body)]
		pub action: PusherAction,
	}

	#[derive(Clone, Debug, Default)]
	pub struct Response {
		pub needs_activation: bool,
	}

	impl Request {
		#[must_use]
		pub fn new(action: PusherAction) -> Self { Self { action } }
	}

	impl Response {
		#[must_use]
		pub fn new(needs_activation: bool) -> Self { Self { needs_activation } }
	}

	#[cfg(feature = "server")]
	impl OutgoingResponse for Response {
		fn try_into_http_response<T: Default + BufMut>(
			self,
		) -> Result<http::Response<T>, IntoHttpError> {
			let status = if self.needs_activation {
				http::StatusCode::CREATED
			} else {
				http::StatusCode::OK
			};

			let mut body = T::default();
			body.put_slice(b"{}");

			http::Response::builder()
				.status(status)
				.header(http::header::CONTENT_TYPE, ruma::http_headers::APPLICATION_JSON)
				.body(body)
				.map_err(Into::into)
		}
	}

	#[cfg(feature = "client")]
	impl IncomingResponse for Response {
		type EndpointError = MatrixError;

		fn try_from_http_response_inner(
			response: http::Response<&[u8]>,
		) -> Result<Self, DeserializationError> {
			Ok(Self {
				needs_activation: response.status() == http::StatusCode::CREATED,
			})
		}
	}

	#[derive(Clone, Debug)]
	pub enum PusherAction {
		Post(Box<Pusher>),
		Delete(PusherIds),
	}

	#[derive(Debug, Deserialize)]
	struct PusherActionDeHelper {
		kind: JsOption<String>,
	}

	impl<'de> Deserialize<'de> for PusherAction {
		fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
		where
			D: de::Deserializer<'de>,
		{
			let json = Box::<RawJsonValue>::deserialize(deserializer)?;
			let PusherActionDeHelper { kind } = from_raw_json_value(&json)?;

			match kind {
				| JsOption::Some(_) => Ok(Self::Post(from_raw_json_value(&json)?)),
				| JsOption::Null => Ok(Self::Delete(from_raw_json_value(&json)?)),
				| JsOption::Undefined => Err(de::Error::missing_field("kind")),
			}
		}
	}

	impl Serialize for PusherAction {
		fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
		where
			S: serde::Serializer,
		{
			match self {
				| Self::Post(pusher) => pusher.serialize(serializer),
				| Self::Delete(ids) => {
					use serde::ser::SerializeStruct;

					let mut st = serializer.serialize_struct("PusherAction", 3)?;
					st.serialize_field("pushkey", &ids.pushkey)?;
					st.serialize_field("app_id", &ids.app_id)?;
					st.serialize_field("kind", &None::<&str>)?;
					st.end()
				},
			}
		}
	}
}
