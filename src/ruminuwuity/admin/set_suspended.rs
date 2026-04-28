//! `PUT /_matrix/client/v1/admin/suspend/{userId}`
//!
//! Set the suspension status of a target user

pub mod v1 {
	//! `/_matrix/client/unstable/uk.timedout.msc4323/admin/suspend/{userID}`
	//! ([msc])
	//!
	//! [msc]: https://github.com/matrix-org/matrix-spec-proposals/pull/4323

	use ruma::{
		OwnedUserId,
		api::{auth_scheme::AccessToken, request, response},
		metadata,
	};

	metadata! {
		method: PUT,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			unstable => "/_matrix/client/unstable/uk.timedout.msc4323/admin/suspend/{user_id}",
			1.18 => "/_matrix/client/v1/admin/suspend/{user_id}",
		}
	}

	/// Request type for the set user suspension status endpoint.
	#[request(error = ruma::api::error::Error)]
	pub struct Request {
		/// The user to look up.
		#[ruma_api(path)]
		pub user_id: OwnedUserId,

		pub suspended: bool,
	}

	/// Response type for the suspension endpoints
	#[response(error = ruma::api::error::Error)]
	pub struct Response {
		/// Whether the user is currently suspended.
		pub suspended: bool,
	}

	impl Request {
		/// Creates a new `Request` with the given user id.
		#[must_use]
		pub fn new(user_id: OwnedUserId, suspended: bool) -> Self { Self { user_id, suspended } }
	}

	impl Response {
		/// Creates a new `Response` with the given suspension status.
		#[must_use]
		pub fn new(suspended: bool) -> Self { Self { suspended } }
	}
}
