use std::hash::RandomState;

use clap::Subcommand;
use conduwuit::{Err, Result, info};
use conduwuit_macros::admin_command_dispatch;
use ruma::{OwnedRoomOrAliasId, OwnedServerName, OwnedUserId};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum RoomMembershipCommands {
	/// Make a local user join a room.
	#[clap(name = "join")]
	JoinRoom {
		/// The user to make join the room. Must be a local user.
		user_id: OwnedUserId,
		/// The room (ID or alias) to join.
		room: OwnedRoomOrAliasId,
		/// The server to join through, if a remote room.
		///
		/// Residents returned after resolving an alias override this option.
		#[arg(long)]
		via: Option<OwnedServerName>,
	},
	/// Make a local user leave a room or reject an invite.
	#[clap(name = "leave", alias = "reject-invite")]
	LeaveRoom {
		/// The user to make leave the room. Must be a local user.
		user_id: OwnedUserId,
		/// The room (ID or alias) to leave.
		room: OwnedRoomOrAliasId,
		/// The server to leave through, if a room we are not a member of (e.g.
		/// an invite).
		///
		/// Residents returned after resolving an alias override this option.
		#[arg(long)]
		via: Option<OwnedServerName>,
	},
}

impl crate::Context<'_> {
	pub(super) async fn join_room(
		&self,
		user_id: OwnedUserId,
		room: OwnedRoomOrAliasId,
		via: Option<OwnedServerName>,
	) -> Result {
		self.bail_restricted()?;
		if !self.services.globals.user_is_local(&user_id) {
			return Err!("User {user_id:?} does not belong to our server.");
		}
		let (room_id, residents) = self
			.services
			.rooms
			.alias
			.resolve_with_servers(&room, via.map(|v| vec![v]))
			.await?;
		self.services
			.rooms
			.membership
			.join_room(&user_id, &room_id, None, &residents)
			.await?;
		self.write_str(&format!("Successfully joined {room_id} as {user_id}."))
			.await
	}

	pub(super) async fn leave_room(
		&self,
		user_id: OwnedUserId,
		room: OwnedRoomOrAliasId,
		via: Option<OwnedServerName>,
	) -> Result {
		self.bail_restricted()?;
		if !self.services.globals.user_is_local(&user_id) {
			return Err!("User {user_id:?} does not belong to our server.");
		}
		let no_explicit_via = via.is_none();
		let (room_id, residents) = self
			.services
			.rooms
			.alias
			.resolve_with_servers(&room, via.map(|v| vec![v]))
			.await?;
		if no_explicit_via
			|| self
				.services
				.rooms
				.state_cache
				.server_in_room(self.services.globals.server_name(), &room_id)
				.await
		{
			if !self
				.services
				.rooms
				.state_cache
				.is_joined(&user_id, &room_id)
				.await
			{
				return Err!("User {user_id:?} is not a member of room {room_id:?}.");
			}

			info!("Leaving room locally...");
			conduwuit_api::client::leave_room(self.services, &user_id, &room_id, None).await?;
		} else {
			info!("Leaving room remotely...");
			conduwuit_api::client::remote_leave_room::<RandomState>(
				self.services,
				&user_id,
				&room_id,
				None,
				residents.into_iter().collect(),
			)
			.await?;
		}
		self.write_str(&format!("Successfully made {user_id} leave {room_id}."))
			.await
	}
}
