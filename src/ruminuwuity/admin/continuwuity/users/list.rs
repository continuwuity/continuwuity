pub mod v1 {
	use ruma::{
		OwnedUserId,
		api::{auth_scheme::AccessToken, request, response},
		metadata,
	};
	use serde::Deserialize;

	metadata! {
		method: GET,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_continuwuity/admin/v1/users",
		}
	}

	#[request]
	#[derive(Default)]
	pub struct Request {
		/// If true, includes deactivated users in the response.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub include_deactivated: bool,
		/// If true, includes locked users in the response.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub include_locked: bool,
		/// If true, includes suspended users in the response.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub include_suspended: bool,

		/// The maximum number of results to return in this page. Maximum (and
		/// default) is 100.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub limit: Option<usize>,

		/// The number of results to skip over before returning results. Default
		/// is 0.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub offset: Option<usize>,
	}

	#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
	pub struct User {
		/// The full user ID of the user.
		pub user_id: OwnedUserId,

		/// Whether this user is deactivated.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub deactivated: bool,

		/// Whether this user is suspended.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub suspended: bool,

		/// Whether this user is locked.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub locked: bool,

		/// Whether this user is an admin.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub admin: bool,

		/// Whether this user has their login disabled.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub login_disabled: bool,
	}

	impl User {
		#[must_use]
		pub fn new(user_id: OwnedUserId) -> Self {
			Self {
				user_id,
				deactivated: false,
				suspended: false,
				locked: false,
				admin: false,
				login_disabled: false,
			}
		}
	}

	#[response]
	#[derive(Default)]
	pub struct Response {
		pub users: Vec<User>,
	}

	impl Request {
		#[must_use]
		pub fn new() -> Self { Self::default() }
	}

	impl Response {
		#[must_use]
		pub fn new(users: Vec<User>) -> Self { Self { users } }
	}

	#[cfg(test)]
	mod tests {
		use assign::assign;
		use serde_json::json;

		use super::*;

		#[test]
		fn request_defaults() {
			let req = Request::new();
			assert!(!req.include_deactivated && !req.include_locked && !req.include_suspended);
		}

		#[test]
		fn user_serialize_omits_default_values() {
			let user_id = OwnedUserId::try_from("@alice:example.org".to_owned()).unwrap();
			let user = User::new(user_id.clone());

			let expected = json!({ "user_id": user_id.to_string() });
			assert_eq!(serde_json::to_value(&user).expect("failed to serialize user"), expected);

			let suspended_user = assign!(user, {suspended: true});
			let expected2 = json!({ "user_id": "@alice:example.org", "suspended": true});
			assert_eq!(
				serde_json::to_value(&suspended_user).expect("failed to serialize user"),
				expected2
			);
		}

		#[test]
		fn response_defaults() {
			let response = Response::default();
			assert!(response.users.is_empty());
		}
	}
}
