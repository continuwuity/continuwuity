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
use ruma::{
	CanonicalJsonObject, OwnedRoomId, RoomId, ServerName, UserId,
	api::client::membership::joined_rooms,
	events::{
		StaticEventContent,
		room::member::{MembershipState, RoomMemberEventContent},
	},
};
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
#[tracing::instrument(skip(services), level = "info")]
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
		let room_banned = services.rooms.metadata.is_banned(room_id).await;
		let server_banned = room_id.server_name().is_some_and(|server_name| {
			services.moderation.is_remote_server_forbidden(server_name)
		});
		if room_banned || server_banned {
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

/// Validates that an event returned from a remote server by `/make_*`
/// actually is a membership event with the expected fields.
///
/// Without checking this, the remote server could use the remote membership
/// mechanism to trick our server into signing arbitrary malicious events.
pub(crate) fn validate_remote_member_event_stub(
	membership: &MembershipState,
	user_id: &UserId,
	room_id: &RoomId,
	event_stub: &CanonicalJsonObject,
) -> Result<()> {
	let Some(event_type) = event_stub.get("type") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing type field"
		));
	};
	if event_type != &RoomMemberEventContent::TYPE {
		return Err!(BadServerResponse(
			"Remote server returned member event with invalid event type"
		));
	}

	let Some(sender) = event_stub.get("sender") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing sender field"
		));
	};
	if sender != &user_id.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect sender"
		));
	}

	let Some(state_key) = event_stub.get("state_key") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing state_key field"
		));
	};
	if state_key != &user_id.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect state_key"
		));
	}

	let Some(event_room_id) = event_stub.get("room_id") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing room_id field"
		));
	};
	if event_room_id != &room_id.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect room_id"
		));
	}

	let Some(content) = event_stub
		.get("content")
		.and_then(|content| content.as_object())
	else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing content field"
		));
	};
	let Some(event_membership) = content.get("membership") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing membership field"
		));
	};
	if event_membership != &membership.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect membership type"
		));
	}

	Ok(())
}
