use axum::extract::State;
use conduwuit::{Err, Result, info, utils::ReadyExt, warn};
use futures::{FutureExt, StreamExt};
use ruma::{
	OwnedRoomAliasId, continuwuity_admin_api::rooms,
	events::room::message::RoomMessageEventContent,
};

use crate::{Ruma, client::leave_room};

/// # `PUT /_continuwuity/admin/rooms/{roomID}/ban`
///
/// Bans or unbans a room.
pub(crate) async fn ban_room(
	State(services): State<crate::State>,
	body: Ruma<rooms::ban::v1::Request>,
) -> Result<rooms::ban::v1::Response> {
	let sender_user = body.sender_user();
	if !services.users.is_admin(sender_user).await {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}

	if body.banned {
		// Don't ban again if already banned
		if services.rooms.metadata.is_banned(&body.room_id).await {
			return Err!(Request(InvalidParam("Room is already banned")));
		}
		info!(%sender_user, "Banning room {}", body.room_id);

		services
			.admin
			.notice(&format!("{sender_user} banned {} (ban in progress)", body.room_id))
			.await;

		let mut users = services
			.rooms
			.state_cache
			.room_members(&body.room_id)
			.map(ToOwned::to_owned)
			.ready_filter(|user| services.globals.user_is_local(user))
			.boxed();
		let mut evicted = Vec::new();
		let mut failed_evicted = Vec::new();

		while let Some(ref user_id) = users.next().await {
			info!("Evicting user {} from room {}", user_id, body.room_id);
			match leave_room(&services, user_id, &body.room_id, None)
				.boxed()
				.await
			{
				| Ok(()) => {
					services.rooms.state_cache.forget(&body.room_id, user_id);
					evicted.push(user_id.clone());
				},
				| Err(e) => {
					warn!("Failed to evict user {} from room {}: {}", user_id, body.room_id, e);
					failed_evicted.push(user_id.clone());
				},
			}
		}

		let aliases: Vec<OwnedRoomAliasId> = services
			.rooms
			.alias
			.local_aliases_for_room(&body.room_id)
			.map(ToOwned::to_owned)
			.collect::<Vec<_>>()
			.await;
		for alias in &aliases {
			info!("Removing alias {} for banned room {}", alias, body.room_id);
			services
				.rooms
				.alias
				.remove_alias(alias, &services.globals.server_user)
				.await?;
		}

		services.rooms.directory.set_not_public(&body.room_id); // remove from the room directory
		services.rooms.metadata.ban_room(&body.room_id, true); // prevent further joins
		services.rooms.metadata.disable_room(&body.room_id, true); // disable federation

		services
			.admin
			.notice(&format!(
				"Finished banning {}: Removed {} users ({} failed) and {} aliases",
				body.room_id,
				evicted.len(),
				failed_evicted.len(),
				aliases.len()
			))
			.await;
		if !evicted.is_empty() || !failed_evicted.is_empty() || !aliases.is_empty() {
			let msg = services
				.admin
				.text_or_file(RoomMessageEventContent::text_markdown(format!(
					"Removed users:\n{}\n\nFailed to remove users:\n{}\n\nRemoved aliases: {}",
					evicted
						.iter()
						.map(|u| u.as_str())
						.collect::<Vec<_>>()
						.join("\n"),
					failed_evicted
						.iter()
						.map(|u| u.as_str())
						.collect::<Vec<_>>()
						.join("\n"),
					aliases
						.iter()
						.map(|a| a.as_str())
						.collect::<Vec<_>>()
						.join(", "),
				)))
				.await;
			services.admin.send_message(msg).await.ok();
		}

		Ok(rooms::ban::v1::Response::new(evicted, failed_evicted, aliases))
	} else {
		// Don't unban if not banned
		if !services.rooms.metadata.is_banned(&body.room_id).await {
			return Err!(Request(InvalidParam("Room is not banned")));
		}
		info!(%sender_user, "Unbanning room {}", body.room_id);
		services.rooms.metadata.disable_room(&body.room_id, false);
		services.rooms.metadata.ban_room(&body.room_id, false);
		services
			.admin
			.notice(&format!("{sender_user} unbanned {}", body.room_id))
			.await;
		Ok(rooms::ban::v1::Response::new(Vec::new(), Vec::new(), Vec::new()))
	}
}
