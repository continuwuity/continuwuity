pub mod v1 {
	use ruma::{
		OwnedRoomId,
		api::{auth_scheme::AccessToken, request, response},
		metadata,
	};

	metadata! {
		method: GET,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			unstable("org.continuwuity.admin") => "/_continuwuity/admin/rooms/list",
			1.0 => "/_continuwuity/admin/v1/rooms",
		}
	}

	#[request]
	#[derive(Default)]
	pub struct Request;

	#[response]
	pub struct Response {
		/// A list of room IDs known to this server.
		pub rooms: Vec<OwnedRoomId>,
	}

	impl Request {
		#[must_use]
		pub fn new() -> Self { Self::default() }
	}

	impl Response {
		#[must_use]
		pub fn new(rooms: Vec<OwnedRoomId>) -> Self { Self { rooms } }
	}
}
