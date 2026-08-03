//! `GET /_matrix/federation/*/pong`
//!
//! Respond to a federation ping
pub mod unstable {
	use ruma::{
		api::{federation::authentication::ServerSignatures, request, response},
		metadata,
	};

	metadata! {
		method: POST,
		rate_limited: false,
		authentication: ServerSignatures,
		path: "/_matrix/federation/unstable/uk.timedout.msc4524.tabletennis/ping"
	}

	#[request]
	pub struct Request {
		pub question: String,
	}

	#[response]
	#[derive(Default)]
	pub struct Response;

	impl Request {
		#[must_use]
		pub fn new(question: String) -> Self { Self { question } }
	}
}
