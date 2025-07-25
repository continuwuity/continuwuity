mod ban;
mod forget;
mod invite;
mod join;
mod kick;
mod knock;
mod leave;
mod members;
mod unban;

use std::net::IpAddr;

use axum::extract::State;
use conduwuit::{Err, Result, warn};
use futures::{FutureExt, StreamExt};
use ruma::{OwnedRoomId, RoomId, ServerName, UserId, api::client::membership::joined_rooms};
use service::Services;

pub(crate) use self::{
	ban::ban_user_route,
	forget::forget_room_route,
	invite::{invite_helper, invite_user_route},
	join::{join_room_by_id_or_alias_route, join_room_by_id_route},
	kick::kick_user_route,
	knock::knock_room_route,
	leave::leave_room_route,
	members::{get_member_events_route, joined_members_route},
	unban::unban_user_route,
};
pub use self::{
	join::join_room_by_id_helper,
	leave::{leave_all_rooms, leave_room, remote_leave_room},
};
use crate::{Ruma, client::full_user_deactivate};

/// # `POST /_matrix/client/r0/joined_rooms`
///
/// Lists all rooms the user has joined.
pub(crate) async fn joined_rooms_route(
	State(services): State<crate::State>,
	body: Ruma<joined_rooms::v3::Request>,
) -> Result<joined_rooms::v3::Response> {
	Ok(joined_rooms::v3::Response {
		joined_rooms: services
			.rooms
			.state_cache
			.rooms_joined(body.sender_user())
			.map(ToOwned::to_owned)
			.collect()
			.await,
	})
}

/// Checks if the room is banned in any way possible and the sender user is not
/// an admin.
///
/// Performs automatic deactivation if `auto_deactivate_banned_room_attempts` is
/// enabled
#[tracing::instrument(skip(services))]
pub(crate) async fn banned_room_check(
	services: &Services,
	user_id: &UserId,
	room_id: Option<&RoomId>,
	server_name: Option<&ServerName>,
	client_ip: IpAddr,
) -> Result {
	if services.users.is_admin(user_id).await {
		return Ok(());
	}

	if let Some(room_id) = room_id {
		if services.rooms.metadata.is_banned(room_id).await
			|| (room_id.server_name().is_some()
				&& services
					.moderation
					.is_remote_server_forbidden(room_id.server_name().expect("legacy room mxid")))
		{
			warn!(
				"User {user_id} who is not an admin attempted to send an invite for or \
				 attempted to join a banned room or banned room server name: {room_id}"
			);

			if services.server.config.auto_deactivate_banned_room_attempts {
				warn!(
					"Automatically deactivating user {user_id} due to attempted banned room join"
				);

				if services.server.config.admin_room_notices {
					services
						.admin
						.send_text(&format!(
							"Automatically deactivating user {user_id} due to attempted banned \
							 room join from IP {client_ip}"
						))
						.await;
				}

				let all_joined_rooms: Vec<OwnedRoomId> = services
					.rooms
					.state_cache
					.rooms_joined(user_id)
					.map(Into::into)
					.collect()
					.await;

				full_user_deactivate(services, user_id, &all_joined_rooms)
					.boxed()
					.await?;
			}

			return Err!(Request(Forbidden("This room is banned on this homeserver.")));
		}
	} else if let Some(server_name) = server_name {
		if services
			.config
			.forbidden_remote_server_names
			.is_match(server_name.host())
		{
			warn!(
				"User {user_id} who is not an admin tried joining a room which has the server \
				 name {server_name} that is globally forbidden. Rejecting.",
			);

			if services.server.config.auto_deactivate_banned_room_attempts {
				warn!(
					"Automatically deactivating user {user_id} due to attempted banned room join"
				);

				if services.server.config.admin_room_notices {
					services
						.admin
						.send_text(&format!(
							"Automatically deactivating user {user_id} due to attempted banned \
							 room join from IP {client_ip}"
						))
						.await;
				}

				let all_joined_rooms: Vec<OwnedRoomId> = services
					.rooms
					.state_cache
					.rooms_joined(user_id)
					.map(Into::into)
					.collect()
					.await;

				full_user_deactivate(services, user_id, &all_joined_rooms)
					.boxed()
					.await?;
			}

			return Err!(Request(Forbidden("This remote server is banned on this homeserver.")));
		}
	}

	Ok(())
}
