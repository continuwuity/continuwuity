//! `POST /_meowlnir/antispam/*/accept_make_join`
//!
//! Endpoint to accept or decline incoming make_join federation requests.
//! Used by the `fi.mau.spam_check` restricted join rule.
//!
//! References:
//! - https://mau.dev/maunium/synapse/-/blob/52741d3/synapse/handlers/event_auth.py#L280-292

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
			1.0 => "/_meowlnir/antispam/{management_room_id}/accept_make_join",
		}
	}

	/// Request type for the `accept_make_join` callback.
	#[request]
	pub struct Request {
		/// The relevant management room
		#[ruma_api(path)]
		pub management_room_id: OwnedRoomId,
		/// The user trying to join a room
		pub user: OwnedUserId,
		/// The room the user is trying to join
		pub room: OwnedRoomId,
	}

	/// Response type for the `accept_make_join` callback.
	#[response]
	#[derive(Default)]
	pub struct Response;

	impl Request {
		/// Creates a new empty `Request`.
		#[must_use]
		pub fn new(
			management_room_id: OwnedRoomId,
			user: OwnedUserId,
			room: OwnedRoomId,
		) -> Self {
			Self { management_room_id, user, room }
		}
	}

	impl Response {
		/// Creates a new empty `Response`.
		#[must_use]
		pub fn new() -> Self { Self::default() }
	}
}
