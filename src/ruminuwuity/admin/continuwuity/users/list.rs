pub mod v1 {
	use std::collections::BTreeMap;

	use ruma::{
		OwnedMxcUri, OwnedUserId,
		api::{OAuthClientScope, auth_scheme::AccessToken, request, response},
		metadata,
	};
	use serde::{Deserialize, Serialize};

	metadata! {
		method: GET,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_continuwuity/admin/v1/users",
		},
		required_client_scopes: [OAuthClientScope::ServerAdministration]
	}

	#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq, Copy)]
	pub enum QueryFilter {
		/// Always include entities matching this criteria.
		#[serde(rename = "include")]
		Include,
		/// Never include entities matching this criteria.
		#[serde(rename = "exclude")]
		Exclude,
	}

	#[derive(Default)]
	#[request]
	#[serde(default)]
	pub struct Request {
		#[ruma_api(query)]
		#[serde(skip_serializing_if = "Option::is_none")]
		pub active: Option<QueryFilter>,
		#[ruma_api(query)]
		#[serde(skip_serializing_if = "Option::is_none")]
		pub deactivated: Option<QueryFilter>,
		#[ruma_api(query)]
		#[serde(skip_serializing_if = "Option::is_none")]
		pub suspended: Option<QueryFilter>,
		#[ruma_api(query)]
		#[serde(skip_serializing_if = "Option::is_none")]
		pub locked: Option<QueryFilter>,
		#[ruma_api(query)]
		#[serde(skip_serializing_if = "Option::is_none")]
		pub admin: Option<QueryFilter>,
		#[ruma_api(query)]
		#[serde(skip_serializing_if = "Option::is_none")]
		pub limit: Option<usize>,
		#[ruma_api(query)]
		#[serde(skip_serializing_if = "Option::is_none")]
		pub page: Option<usize>,
	}

	#[derive(Default, Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
	pub struct User {
		#[serde(skip_serializing_if = "Option::is_none")]
		pub displayname: Option<String>,
		#[serde(skip_serializing_if = "Option::is_none")]
		pub avatar_url: Option<OwnedMxcUri>,
		#[serde(skip_serializing_if = "ruma::serde::is_default")]
		pub suspended: bool,
		#[serde(skip_serializing_if = "ruma::serde::is_default")]
		pub locked: bool,
		#[serde(skip_serializing_if = "ruma::serde::is_default")]
		pub deactivated: bool,
		#[serde(skip_serializing_if = "ruma::serde::is_default")]
		pub admin: bool,
	}

	#[response]
	pub struct Response {
		#[ruma_api(body)]
		pub users: BTreeMap<OwnedUserId, User>,
	}

	impl Response {
		#[must_use]
		pub fn new(users: BTreeMap<OwnedUserId, User>) -> Self { Self { users } }
	}
}
