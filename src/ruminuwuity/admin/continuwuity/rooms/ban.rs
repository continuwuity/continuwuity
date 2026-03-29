pub mod v1 {
	use ruma::{OwnedRoomAliasId, OwnedRoomId, OwnedUserId, api::{auth_scheme::AccessToken, request, response}, metadata};

	metadata! {
		method: PUT,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_continuwuity/admin/rooms/{room_id}/ban",
		}
	}

	#[request]
	pub struct Request {
		#[ruma_api(path)]
		pub room_id: OwnedRoomId,

		/// Whether to ban (true) or unban (false) the room.
		/// If true, and the room is not banned, all local users will be evacuated
		/// and prevented from re-joining.
		/// If false, and the room is unbanned, local users will be allowed to re-join.
		/// No-ops are no-ops.
		pub banned: bool,
	}

	#[response]
	pub struct Response {
		pub kicked_users: Vec<OwnedUserId>,
		pub failed_kicked_users: Vec<OwnedUserId>,
		pub local_aliases: Vec<OwnedRoomAliasId>
	}

	impl Request {
        #[must_use]
		pub fn new(room_id: OwnedRoomId, banned: bool) -> Self {
			Self { room_id, banned }
		}
	}

	impl Response {
        #[must_use]
		pub fn new(
			kicked_users: Vec<OwnedUserId>,
			failed_kicked_users: Vec<OwnedUserId>,
			local_aliases: Vec<OwnedRoomAliasId>,
		) -> Self {
			Self { kicked_users, failed_kicked_users, local_aliases }
		}
	}
}