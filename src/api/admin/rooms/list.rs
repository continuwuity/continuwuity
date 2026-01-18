use axum::extract::State;
use conduwuit::{Err, Result};
use futures::StreamExt;
use ruma::{OwnedRoomId, continuwuity_admin_api::rooms};

use crate::Ruma;

/// # `GET /_continuwuity/admin/rooms/list`
///
/// Lists all rooms known to this server, excluding banned ones.
pub(crate) async fn list_rooms(
	State(services): State<crate::State>,
	body: Ruma<rooms::list::v1::Request>,
) -> Result<rooms::list::v1::Response> {
	let sender_user = body.sender_user();
	if !services.users.is_admin(sender_user).await {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}

	let mut rooms: Vec<OwnedRoomId> = services
		.rooms
		.metadata
		.iter_ids()
		.filter_map(|room_id| async move {
			if !services.rooms.metadata.is_banned(room_id).await {
				Some(room_id.to_owned())
			} else {
				None
			}
		})
		.collect()
		.await;
	rooms.sort();
	Ok(rooms::list::v1::Response::new(rooms))
}
