pub mod unstable {
	use ruma::{
		api::{auth_scheme::AccessToken, error::Error as MatrixError, request, response},
		metadata,
	};

	metadata! {
		method: POST,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			unstable => "/_matrix/client/unstable/org.matrix.msc4174/pushers/ack",
		}
	}

	#[request(error = MatrixError)]
	pub struct Request {
		pub app_id: String,
		pub ack_token: String,
	}

	#[response(error = MatrixError)]
	#[derive(Default)]
	#[allow(clippy::empty_structs_with_brackets, reason = "required by #[response]")]
	pub struct Response {}

	impl Request {
		#[must_use]
		pub fn new(app_id: String, ack_token: String) -> Self { Self { app_id, ack_token } }
	}

	impl Response {
		#[must_use]
		pub fn new() -> Self { Self {} }
	}
}
