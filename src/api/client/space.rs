use axum::extract::State;
use conduwuit::{Err, Result};
use ruma::{UInt, api::client::space::get_hierarchy, assign};
use service::rooms::summary::Accessibility;

use crate::Ruma;

const MAX_MAX_DEPTH: u32 = 10;

/// # `GET /_matrix/client/v1/rooms/{room_id}/hierarchy`
///
/// Paginates over the space tree in a depth-first manner to locate child rooms
/// of a given space.
pub(crate) async fn get_hierarchy_route(
	State(services): State<crate::State>,
	body: Ruma<get_hierarchy::v1::Request>,
) -> Result<get_hierarchy::v1::Response> {
	// We don't do pagination for this route (and therefore ignore `limit`), since
	// there's no reasonable way to handle a space hierarchy changing during
	// pagination.

	let max_depth = body
		.max_depth
		.map(|max_depth| max_depth.min(UInt::from(MAX_MAX_DEPTH)));

	let hierarchy = services
		.rooms
		.summary
		.get_room_hierarchy_for_user(
			body.sender_user(),
			body.room_id.clone(),
			max_depth,
			body.suggested_only,
		)
		.await?;

	match hierarchy {
		| Accessibility::Accessible(rooms) =>
			Ok(assign!(get_hierarchy::v1::Response::new(), { rooms: rooms })),
		| Accessibility::Inaccessible => {
			Err!(Request(Forbidden("You may not preview this room."), FORBIDDEN))
		},
		| Accessibility::NotFound => {
			Err!(Request(Forbidden("This room does not exist."), FORBIDDEN))
		},
	}
}
