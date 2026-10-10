pub mod v1 {

	use ruma::{
		OwnedUserId,
		api::{OAuthClientScope, auth_scheme::AccessToken, request, response},
		metadata,
		profile::UserProfile,
	};

	metadata! {
		method: POST,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_continuwuity/admin/v1/users",
		},
		required_client_scopes: [OAuthClientScope::ServerAdministration]
	}

	#[request]
	pub struct Request {
		/// The full user ID to create. Must belong to the server.
		pub user_id: OwnedUserId,
		/// The password to set for the new user.
		///
		/// If `None`, the user will not be able to log in.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub password: Option<String>,
		/// The profile fields to set for the user. This can be used to set the
		/// new user's display name and profile picture.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub profile: Option<UserProfile>,
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub admin: bool,
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub suspended: bool,
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub locked: bool,
	}

	#[derive(Default)]
	#[response]
	pub struct Response;

	impl Request {
		#[must_use]
		pub fn new(user_id: OwnedUserId) -> Self {
			Self {
				user_id,
				password: None,
				profile: None,
				admin: false,
				suspended: false,
				locked: false,
			}
		}
	}
}
