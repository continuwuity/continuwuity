use axum::extract::State;
use conduwuit::{Error, Result};
use futures::StreamExt;
use ruma::api::client::{error::ErrorKind, room::aliases};

use crate::Ruma;

/// # `GET /_matrix/client/r0/rooms/{roomId}/aliases`
///
/// Lists all aliases of the room.
///
/// - Only users joined to the room are allowed to call this, or if
///   `history_visibility` is world readable in the room
pub(crate) async fn get_room_aliases_route(
	State(services): State<crate::State>,
	body: Ruma<aliases::v3::Request>,
) -> Result<aliases::v3::Response> {
	let sender_user = body.sender_user();

	if !services
		.rooms
		.state_accessor
		.user_can_see_state_events(sender_user, &body.room_id)
		.await
	{
		return Err(Error::BadRequest(
			ErrorKind::forbidden(),
			"You don't have permission to view this room.",
		));
	}

	Ok(aliases::v3::Response {
		aliases: services
			.rooms
			.alias
			.local_aliases_for_room(&body.room_id)
			.map(ToOwned::to_owned)
			.collect()
			.await,
	})
}
