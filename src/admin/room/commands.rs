use conduwuit::{Err, Result};
use futures::StreamExt;
use ruma::{OwnedRoomId, OwnedRoomOrAliasId};

use crate::{PAGE_SIZE, admin_command, get_room_info};

#[allow(clippy::fn_params_excessive_bools)]
#[admin_command]
pub(super) async fn list_rooms(
	&self,
	page: Option<usize>,
	exclude_disabled: bool,
	exclude_banned: bool,
	include_empty: bool,
	no_details: bool,
) -> Result {
	// TODO: i know there's a way to do this with clap, but i can't seem to find it
	let page = page.unwrap_or(1);
	let mut rooms = self
		.services
		.rooms
		.metadata
		.iter_ids()
		.filter_map(|room_id| async move {
			(!exclude_disabled || !self.services.rooms.metadata.is_disabled(room_id).await)
				.then_some(room_id)
		})
		.filter_map(|room_id| async move {
			(!exclude_banned || !self.services.rooms.metadata.is_banned(room_id).await)
				.then_some(room_id)
		})
		.then(|room_id| get_room_info(self.services, room_id))
		.then(|(room_id, total_members, name)| async move {
			let local_members: Vec<_> = self
				.services
				.rooms
				.state_cache
				.active_local_users_in_room(&room_id)
				.collect()
				.await;
			let local_members = local_members.len();
			(room_id, total_members, local_members, name)
		})
		.filter_map(|(room_id, total_members, local_members, name)| async move {
			(include_empty || local_members > 0).then_some((room_id, total_members, name))
		})
		.collect::<Vec<_>>()
		.await;

	rooms.sort_by_key(|r| r.1);
	rooms.reverse();

	let rooms = rooms
		.into_iter()
		.skip(page.saturating_sub(1).saturating_mul(PAGE_SIZE))
		.take(PAGE_SIZE)
		.collect::<Vec<_>>();

	if rooms.is_empty() {
		return Err!("No more rooms.");
	}

	let body = rooms
		.iter()
		.map(|(id, members, name)| {
			if no_details {
				format!("{id}")
			} else {
				format!("{id}\tMembers: {members}\tName: {name}")
			}
		})
		.collect::<Vec<_>>()
		.join("\n");

	self.write_str(&format!("Rooms ({}):\n```\n{body}\n```", rooms.len(),))
		.await
}

#[admin_command]
pub(super) async fn exists(&self, room_id: OwnedRoomId) -> Result {
	let result = self.services.rooms.metadata.exists(&room_id).await;

	self.write_str(&format!("{result}")).await
}

#[admin_command]
pub(super) async fn purge_sync_tokens(&self, room: OwnedRoomOrAliasId) -> Result {
	// Resolve the room ID from the room or alias ID
	let room_id = self.services.rooms.alias.resolve(&room).await?;

	// Delete all tokens for this room using the service method
	let deleted_count = match self.services.rooms.user.delete_room_tokens(&room_id).await {
		| Ok(count) => count,
		| Err(_) => return Err!("Failed to delete sync tokens for room {}", room_id),
	};

	self.write_str(&format!(
		"Successfully deleted {deleted_count} sync tokens for room {room_id}"
	))
	.await
}
