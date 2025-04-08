use clap::Subcommand;
use conduwuit::Result;
use futures::StreamExt;
use ruma::{OwnedRoomId, events::room::message::RoomMessageEventContent};

use crate::{Command, PAGE_SIZE, get_room_info};

#[derive(Debug, Subcommand)]
pub(crate) enum RoomDirectoryCommand {
	/// - Publish a room to the room directory
	Publish {
		/// The room id of the room to publish
		room_id: OwnedRoomId,
	},

	/// - Unpublish a room to the room directory
	Unpublish {
		/// The room id of the room to unpublish
		room_id: OwnedRoomId,
	},

	/// - List rooms that are published
	List {
		page: Option<usize>,
	},
}

pub(super) async fn process(command: RoomDirectoryCommand, context: &Command<'_>) -> Result {
	let c = reprocess(command, context).await?;
	context.write_str(c.body()).await?;
	Ok(())
}

pub(super) async fn reprocess(
	command: RoomDirectoryCommand,
	context: &Command<'_>,
) -> Result<RoomMessageEventContent> {
	let services = context.services;
	match command {
		| RoomDirectoryCommand::Publish { room_id } => {
			services.rooms.directory.set_public(&room_id);
			Ok(RoomMessageEventContent::notice_plain("Room published"))
		},
		| RoomDirectoryCommand::Unpublish { room_id } => {
			services.rooms.directory.set_not_public(&room_id);
			Ok(RoomMessageEventContent::notice_plain("Room unpublished"))
		},
		| RoomDirectoryCommand::List { page } => {
			// TODO: i know there's a way to do this with clap, but i can't seem to find it
			let page = page.unwrap_or(1);
			let mut rooms: Vec<_> = services
				.rooms
				.directory
				.public_rooms()
				.then(|room_id| get_room_info(services, room_id))
				.collect()
				.await;

			rooms.sort_by_key(|r| r.1);
			rooms.reverse();

			let rooms: Vec<_> = rooms
				.into_iter()
				.skip(page.saturating_sub(1).saturating_mul(PAGE_SIZE))
				.take(PAGE_SIZE)
				.collect();

			if rooms.is_empty() {
				return Ok(RoomMessageEventContent::text_plain("No more rooms."));
			}

			let output = format!(
				"Rooms (page {page}):\n```\n{}\n```",
				rooms
					.iter()
					.map(|(id, members, name)| format!(
						"{id} | Members: {members} | Name: {name}"
					))
					.collect::<Vec<_>>()
					.join("\n")
			);
			Ok(RoomMessageEventContent::text_markdown(output))
		},
	}
}
