//! `POST /_meowlnir/antispam/*/user_may_invite`
//!
//! Checks that a user may invite the given user to the given room via Meowlnir
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
			1.0 => "/_meowlnir/antispam/{management_room_id}/user_may_invite",
		}
	}

	/// Request type for the `user_may_invite` callback.
	#[request]
	pub struct Request {
		/// The relevant management room
		#[ruma_api(path)]
		pub management_room_id: OwnedRoomId,
		/// The user sending the invite
		pub inviter: OwnedUserId,
		/// The user being invited
		pub invitee: OwnedUserId,
		/// The room the invitee is being invited to
		pub room_id: OwnedRoomId,
	}

	/// Response type for the `user_may_invite` callback.
	#[response]
	#[derive(Default)]
	pub struct Response;

	impl Request {
		/// Creates a new empty `Request`.
		#[must_use]
		pub fn new(
			management_room_id: OwnedRoomId,
			inviter: OwnedUserId,
			invitee: OwnedUserId,
			room_id: OwnedRoomId,
		) -> Self {
			Self {
				management_room_id,
				inviter,
				invitee,
				room_id,
			}
		}
	}

	impl Response {
		/// Creates a new empty `Response`.
		#[must_use]
		pub fn new() -> Self { Self::default() }
	}
}
