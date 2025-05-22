use conduwuit::{Err, Result};
use futures::StreamExt;
use ruma::{OwnedRoomId, OwnedRoomOrAliasId};

use crate::{PAGE_SIZE, admin_command, get_room_info};

#[admin_command]
pub(super) async fn list_rooms(
	&self,
	page: Option<usize>,
	exclude_disabled: bool,
	exclude_banned: bool,
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
	let Ok(deleted_count) = self.services.rooms.user.delete_room_tokens(&room_id).await else {
		return Err!("Failed to delete sync tokens for room {}", room_id);
	};

	self.write_str(&format!(
		"Successfully deleted {deleted_count} sync tokens for room {room_id}"
	))
	.await
}

/// Target options for room purging
#[derive(Default, Debug, clap::ValueEnum, Clone)]
pub(crate) enum RoomTargetOption {
	#[default]
	/// Target all rooms
	All,
	/// Target only disabled rooms
	DisabledOnly,
	/// Target only banned rooms
	BannedOnly,
}

#[admin_command]
pub(super) async fn purge_all_sync_tokens(
	&self,
	target_option: Option<RoomTargetOption>,
	execute: bool,
) -> Result {
	use conduwuit::{debug, info};

	let mode = if !execute { "Simulating" } else { "Starting" };

	// strictly, we should check if these reach the max value after the loop and
	// warn the user that the count is too large
	let mut total_rooms_checked: usize = 0;
	let mut total_tokens_deleted: usize = 0;
	let mut error_count: u32 = 0;
	let mut skipped_rooms: usize = 0;

	info!("{} purge of sync tokens", mode);

	// Get all rooms in the server
	let all_rooms = self
		.services
		.rooms
		.metadata
		.iter_ids()
		.collect::<Vec<_>>()
		.await;

	info!("Found {} rooms total on the server", all_rooms.len());

	// Filter rooms based on options
	let mut rooms = Vec::new();
	for room_id in all_rooms {
		if let Some(target) = &target_option {
			match target {
				| RoomTargetOption::DisabledOnly => {
					if !self.services.rooms.metadata.is_disabled(room_id).await {
						debug!("Skipping room {} as it's not disabled", room_id);
						skipped_rooms = skipped_rooms.saturating_add(1);
						continue;
					}
				},
				| RoomTargetOption::BannedOnly => {
					if !self.services.rooms.metadata.is_banned(room_id).await {
						debug!("Skipping room {} as it's not banned", room_id);
						skipped_rooms = skipped_rooms.saturating_add(1);
						continue;
					}
				},
				| RoomTargetOption::All => {},
			}
		}

		rooms.push(room_id);
	}

	// Total number of rooms we'll be checking
	let total_rooms = rooms.len();
	info!(
		"Processing {} rooms after filtering (skipped {} rooms)",
		total_rooms, skipped_rooms
	);

	// Process each room
	for room_id in rooms {
		total_rooms_checked = total_rooms_checked.saturating_add(1);

		// Log progress periodically
		if total_rooms_checked % 100 == 0 || total_rooms_checked == total_rooms {
			info!(
				"Progress: {}/{} rooms checked, {} tokens {}",
				total_rooms_checked,
				total_rooms,
				total_tokens_deleted,
				if !execute { "would be deleted" } else { "deleted" }
			);
		}

		// In dry run mode, just count what would be deleted, don't actually delete
		debug!(
			"Room {} has no local users, {}",
			room_id,
			if !execute {
				"would purge sync tokens"
			} else {
				"purging sync tokens"
			}
		);

		if !execute {
			// For dry run mode, count tokens without deleting
			match self.services.rooms.user.count_room_tokens(room_id).await {
				| Ok(count) =>
					if count > 0 {
						debug!("Would delete {} sync tokens for room {}", count, room_id);
						total_tokens_deleted = total_tokens_deleted.saturating_add(count);
					} else {
						debug!("No sync tokens found for room {}", room_id);
					},
				| Err(e) => {
					debug!("Error counting sync tokens for room {}: {:?}", room_id, e);
					error_count = error_count.saturating_add(1);
				},
			}
		} else {
			// Real deletion mode
			match self.services.rooms.user.delete_room_tokens(room_id).await {
				| Ok(count) =>
					if count > 0 {
						debug!("Deleted {} sync tokens for room {}", count, room_id);
						total_tokens_deleted = total_tokens_deleted.saturating_add(count);
					} else {
						debug!("No sync tokens found for room {}", room_id);
					},
				| Err(e) => {
					debug!("Error purging sync tokens for room {}: {:?}", room_id, e);
					error_count = error_count.saturating_add(1);
				},
			}
		}
	}

	let action = if !execute { "would be deleted" } else { "deleted" };
	info!(
		"Finished {}: checked {} rooms out of {} total, {} tokens {}, errors: {}",
		if !execute {
			"purge simulation"
		} else {
			"purging sync tokens"
		},
		total_rooms_checked,
		total_rooms,
		total_tokens_deleted,
		action,
		error_count
	);

	self.write_str(&format!(
		"Finished {}: checked {} rooms out of {} total, {} tokens {}, errors: {}",
		if !execute { "simulation" } else { "purging sync tokens" },
		total_rooms_checked,
		total_rooms,
		total_tokens_deleted,
		action,
		error_count
	))
	.await
}
