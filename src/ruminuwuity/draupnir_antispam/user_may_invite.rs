//! `POST /api/1/spam_check/user_may_invite`
//!
//! Checks that a user may invite the given user to the given room via Draupnir
//! anti-spam

pub mod v1 {
	use ruma::{
		OwnedRoomId, OwnedUserId,
		api::{auth_scheme::AppserviceToken, request, response},
		metadata,
	};

	metadata! {
		method: POST,
		rate_limited: false,
		authentication: AppserviceToken,
		history: {
			1.0 => "/api/1/spam_check/user_may_invite",
		}
	}

	/// Request type for the `user_may_invite` callback.
	#[request]
	pub struct Request {
		/// The room the invitee is being invited to
		pub room_id: OwnedRoomId,
		/// The user sending the invite
		pub inviter: OwnedUserId,
		/// The user being invited
		pub invitee: OwnedUserId,
	}

	/// Response type for the `user_may_invite` callback.
	#[response]
	#[derive(Default)]
	pub struct Response;

	impl Request {
		/// Creates a new empty `Request`.
		#[must_use]
		pub fn new(room_id: OwnedRoomId, inviter: OwnedUserId, invitee: OwnedUserId) -> Self {
			Self { room_id, inviter, invitee }
		}
	}

	impl Response {
		/// Creates a new empty `Response`.
		#[must_use]
		pub fn new() -> Self { Self::default() }
	}
}
