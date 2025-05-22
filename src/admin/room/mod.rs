mod alias;
mod commands;
mod directory;
mod info;
mod moderation;

use clap::Subcommand;
use commands::RoomTargetOption;
use conduwuit::Result;
use ruma::{OwnedRoomId, OwnedRoomOrAliasId};

use self::{
	alias::RoomAliasCommand, directory::RoomDirectoryCommand, info::RoomInfoCommand,
	moderation::RoomModerationCommand,
};
use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub(super) enum RoomCommand {
	/// - List all rooms the server knows about
	#[clap(alias = "list")]
	ListRooms {
		page: Option<usize>,

		/// Excludes rooms that we have federation disabled with
		#[arg(long)]
		exclude_disabled: bool,

		/// Excludes rooms that we have banned
		#[arg(long)]
		exclude_banned: bool,

		#[arg(long)]
		/// Whether to only output room IDs without supplementary room
		/// information
		no_details: bool,
	},

	#[command(subcommand)]
	/// - View information about a room we know about
	Info(RoomInfoCommand),

	#[command(subcommand)]
	/// - Manage moderation of remote or local rooms
	Moderation(RoomModerationCommand),

	#[command(subcommand)]
	/// - Manage rooms' aliases
	Alias(RoomAliasCommand),

	#[command(subcommand)]
	/// - Manage the room directory
	Directory(RoomDirectoryCommand),

	/// - Check if we know about a room
	Exists {
		room_id: OwnedRoomId,
	},

	/// - Delete all sync tokens for a room
	PurgeSyncTokens {
		/// Room ID or alias to purge sync tokens for
		#[arg(value_parser)]
		room: OwnedRoomOrAliasId,
	},

	/// - Delete sync tokens for all rooms that have no local users
	///
	/// By default, processes all empty rooms.
	PurgeAllSyncTokens {
		/// Target specific room types
		#[arg(long, value_enum)]
		target_option: Option<RoomTargetOption>,

		/// Execute token deletions. Otherwise,
		/// Performs a dry run without actually deleting any tokens
		#[arg(long)]
		execute: bool,
	},
}
