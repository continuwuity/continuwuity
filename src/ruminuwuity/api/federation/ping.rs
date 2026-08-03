//! `GET /_matrix/federation/*/ping`
//!
//! Initiate a federation ping
pub mod unstable {
	use ruma::{
		OwnedServerName,
		api::{federation::authentication::ServerSignatures, request, response},
		metadata,
	};

	metadata! {
		method: POST,
		rate_limited: true,
		authentication: ServerSignatures,  // TODO: Needs to be optional
		path: "/_matrix/federation/unstable/uk.timedout.msc4524.tabletennis/ping"
	}

	#[request]
	pub struct Request {
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub origin: Option<OwnedServerName>,

		pub question: String,
	}

	#[response]
	pub struct Response {
		pub answer: String,

		#[serde(default, skip_serializing_if = "Vec::is_empty")]
		pub details: Vec<String>,
	}

	impl Request {
		#[must_use]
		pub fn new(question: String) -> Self { Self { question, origin: None } }
	}

	impl Response {
		#[must_use]
		pub fn new(answer: String) -> Self { Self { answer, details: vec![] } }
	}
}
