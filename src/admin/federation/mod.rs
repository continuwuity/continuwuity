mod commands;

use clap::Subcommand;
use conduwuit::Result;
use ruma::{OwnedRoomId, OwnedRoomOrAliasId, OwnedServerName, OwnedUserId};

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum FederationCommand {
	/// List all rooms we are currently handling an incoming pdu from
	#[clap(name = "incoming", alias = "inbound")]
	IncomingFederation,

	/// Disables incoming federation handling for a room.
	///
	/// The room can still be used locally, but events from other servers will
	/// stop being received.
	DisableRoom {
		room: OwnedRoomOrAliasId,
	},

	/// Enables incoming federation handling for a room again.
	///
	/// This does not unban the room if it was banned, it only re-enables
	/// incoming events.
	EnableRoom {
		room: OwnedRoomOrAliasId,
	},

	/// Fetch support details from the given server.
	///
	/// This makes a request to `https://<server_name>/.well-known/matrix/support`. Many servers do not advertise support information, but those that do usually provide a way to contact homeserver administrators.
	FetchSupport {
		server_name: OwnedServerName,
	},

	/// Queries the remote server for their current signing keys.
	GetSigningKeys {
		server_name: OwnedServerName,
	},

	/// Makes a federation request to the remote server, to test connectivity.
	///
	/// The request is made to the federation version endpoint, which if
	/// successful, will also reveal how long the request took and what the
	/// remote server's software is.
	Ping {
		server_name: OwnedServerName,
	},
}
