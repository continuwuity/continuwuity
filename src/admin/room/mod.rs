mod alias;
mod commands;
mod directory;
mod graph;
mod info;
mod moderation;
mod state;

use clap::Subcommand;
use conduwuit::Result;
use ruma::OwnedRoomId;

use self::{
	alias::RoomAliasCommand, directory::RoomDirectoryCommand, graph::GraphCommand,
	info::RoomInfoCommand, moderation::RoomModerationCommand,
};
use crate::{admin_command_dispatch, room::state::RoomStateCommand};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum RoomCommand {
	/// List all rooms the server knows about
	#[command(name = "list", alias = "list-rooms")]
	ListRooms {
		page: Option<usize>,

		/// Excludes rooms that we have federation disabled with
		#[arg(long)]
		exclude_disabled: bool,

		/// Excludes rooms that we have banned
		#[arg(long)]
		exclude_banned: bool,

		/// Includes disconnected/empty rooms (rooms with zero members)
		#[arg(long)]
		include_empty: bool,

		#[arg(long)]
		/// Whether to only output room IDs without supplementary room
		/// information
		no_details: bool,
	},

	/// View information about a room we know about
	#[command(subcommand)]
	Info(RoomInfoCommand),

	/// Manage moderation of remote or local rooms
	#[command(subcommand)]
	Moderation(RoomModerationCommand),

	/// Manage rooms' aliases
	#[command(subcommand)]
	Alias(RoomAliasCommand),

	/// Manage the room directory
	#[command(subcommand)]
	Directory(RoomDirectoryCommand),

	/// Interface with room graphs directly (advanced).
	#[command(subcommand)]
	Graph(GraphCommand),

	/// Interface with room state directly (advanced).
	#[command(subcommand)]
	State(RoomStateCommand),

	/// Check if we know about a room
	Exists {
		room_id: OwnedRoomId,
	},
}
