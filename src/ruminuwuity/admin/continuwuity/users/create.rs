pub mod v1 {
	use ruma::{
		OwnedMxcUri, OwnedRoomOrAliasId, OwnedUserId,
		api::{auth_scheme::AccessToken, request, response},
		metadata,
	};

	metadata! {
		method: POST,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_continuwuity/admin/v1/users/create",
		},
	}

	#[request]
	pub struct Request {
		/// The user's localpart (the identifier between `@` and `:`). Cannot be
		/// blank.
		pub localpart: String,

		/// The user's desired password. Cannot be blank.
		pub password: String,

		/// The user's email address, if any.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub email: Option<String>,

		/// The display name to set upon creation.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub display_name: Option<String>,

		/// The avatar URI to set upon creation.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub avatar_url: Option<OwnedMxcUri>,

		/// Suspends the user immediately upon creation. They can still log in.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub suspended: bool,

		/// Locks the user immediately upon creation. They will receive
		/// M_USER_LOCKED upon login.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub locked: bool,

		/// Disables the user's login immediately upon creation.
		///
		/// The user can still be used if an admin generates an access token for
		/// the account, but the user will not be able to use `POST
		/// /_matrix/client/v3/login`.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub login_disabled: bool,

		/// Promotes the user to a server administrator immediately upon
		/// creation.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub admin: bool,

		/// Skips joining rooms in the server's configured auto_join_rooms.
		///
		/// If this is false, all rooms in the config.toml's `auto_join_rooms`
		/// will be automatically joined upon creation. If `auto_join_rooms`
		/// is supplied in this request too, those rooms will be joined
		/// afterwards.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub skip_auto_join: bool,

		/// Additional rooms to auto-join the new user to. If `skip_auto_join`
		/// is `true`, these rooms will still be joined.
		#[serde(default, skip_serializing_if = "ruma::serde::is_default")]
		pub auto_join_rooms: Vec<OwnedRoomOrAliasId>,
	}

	#[response]
	pub struct Response {
		/// The fully qualified user ID of the newly created user.
		pub user_id: OwnedUserId,
	}

	impl Request {
		#[must_use]
		pub fn new(localpart: String, password: String) -> Self {
			Self {
				localpart,
				password,
				email: None,
				display_name: None,
				avatar_url: None,
				suspended: false,
				locked: false,
				login_disabled: false,
				admin: false,
				skip_auto_join: false,
				auto_join_rooms: Vec::new(),
			}
		}
	}

	impl Response {
		#[must_use]
		pub fn new(user_id: OwnedUserId) -> Self { Self { user_id } }
	}
}
