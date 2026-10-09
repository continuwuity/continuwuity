//! `POST /_matrix/key/*/query`
//!
//! Endpoint to query signing keys through other (notary) servers.

pub mod unstable {
	use std::collections::BTreeMap;

	use ruma::{
		OwnedServerName, OwnedServerSigningKeyId,
		api::{
			federation::{
				authentication::ServerSignatures,
				discovery::{ServerSigningKeys, get_remote_server_keys_batch::v2::QueryCriteria},
			},
			request, response,
		},
		metadata,
		serde::Raw,
	};

	metadata! {
		method: POST,
		rate_limited: true,
		authentication: ServerSignatures,
		history: {
			unstable("org.continuwuity.msc4553") => "/_matrix/key/unstable/org.continuwuity.msc4556/query",
		}
	}

	#[request]
	pub struct Request {
		pub server_keys:
			BTreeMap<OwnedServerName, BTreeMap<OwnedServerSigningKeyId, QueryCriteria>>,
	}

	#[response]
	pub struct Response {
		pub server_keys: Vec<Raw<ServerSigningKeys>>,
	}

	impl Request {
		#[must_use]
		pub fn new(
			server_keys: BTreeMap<
				OwnedServerName,
				BTreeMap<OwnedServerSigningKeyId, QueryCriteria>,
			>,
		) -> Self {
			Self { server_keys }
		}
	}

	impl Response {
		/// Creates a new `Response` with the given keys.
		#[must_use]
		pub fn new(server_keys: Vec<Raw<ServerSigningKeys>>) -> Self { Self { server_keys } }
	}
}
