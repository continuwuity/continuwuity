use std::hash::RandomState;

use clap::Subcommand;
use conduwuit::{Err, Result, info};
use conduwuit_macros::admin_command_dispatch;
use futures::StreamExt;
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

	/// Makes all local users join the designated room.
	JoinAllLocalUsers {
		/// The room (ID or alias) to join.
		room: OwnedRoomOrAliasId,
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

	pub(super) async fn join_all_local_users(&self, room: OwnedRoomOrAliasId) -> Result {
		self.bail_restricted()?;
		let (room_id, residents) = self
			.services
			.rooms
			.alias
			.resolve_with_servers(&room, None)
			.await?;
		if !self
			.services
			.rooms
			.state_cache
			.is_joined(
				self.sender
					.expect("This command must be executed by a user"),
				&room_id,
			)
			.await
		{
			return Err!("You must be a member of the room you want to join everyone to.");
		}
		let mut success = 0_usize;
		let mut failed = 0_usize;
		let mut local_users = self.services.users.stream_local_users();
		while let Some(user_id) = local_users.next().await {
			if self
				.services
				.rooms
				.membership
				.join_room(&user_id, &room_id, None, &residents)
				.await
				.is_ok()
			{
				success = success.saturating_add(1);
			} else {
				failed = failed.saturating_add(1);
			}
		}
		self.write_str(&format!("Done: {success} successful joins, {failed} failed."))
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
