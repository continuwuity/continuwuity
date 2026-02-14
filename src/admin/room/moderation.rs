use std::collections::BTreeMap;

use api::client::leave_room;
use clap::Subcommand;
use conduwuit::{
	Err, Result, RoomVersion, debug, info,
	utils::{IterStream, ReadyExt},
	warn,
};
use futures::{FutureExt, StreamExt};
use ruma::{
	Int, OwnedRoomId, OwnedRoomOrAliasId, RoomAliasId, RoomId, RoomOrAliasId,
	events::{
		StateEventType,
		room::{
			create::RoomCreateEventContent,
			history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
			join_rules::{JoinRule, RoomJoinRulesEventContent},
			member::{MembershipState, RoomMemberEventContent},
			power_levels::RoomPowerLevelsEventContent,
			tombstone::RoomTombstoneEventContent,
		},
	},
	exports::serde::Deserialize,
};
use serde_json::json;

use crate::{admin_command, admin_command_dispatch, get_room_info};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum RoomModerationCommand {
	/// Bans a room from local users joining and evicts all our local users
	///   (including server
	/// admins)
	///   from the room. Also blocks any invites (local and remote) for the
	///   banned room, and disables federation entirely with it.
	BanRoom {
		/// The room in the format of `!roomid:example.com` or a room alias in
		/// the format of `#roomalias:example.com`
		room: OwnedRoomOrAliasId,
	},

	/// Bans a list of rooms (room IDs and room aliases) from a newline
	///   delimited codeblock similar to `user deactivate-all`. Applies the same
	///   steps as ban-room
	BanListOfRooms,

	/// Unbans a room to allow local users to join again
	UnbanRoom {
		/// The room in the format of `!roomid:example.com` or a room alias in
		/// the format of `#roomalias:example.com`
		room: OwnedRoomOrAliasId,
	},

	/// List of all rooms we have banned
	ListBannedRooms {
		#[arg(long)]
		/// Whether to only output room IDs without supplementary room
		/// information
		no_details: bool,
	},

	/// - Take over a room by puppeting a local user into giving you a higher
	///   power level
	Takeover {
		/// Whether to force joining the room if no local users are in the room
		#[arg(long)]
		force: bool,
		/// The room in the format of `!roomid:example.com` or a room alias in
		/// the format of `#roomalias:example.com`
		room: OwnedRoomOrAliasId,
	},

	/// - Shut down a room, as much is possible. **This is immediate and
	///   irreversible**.
	///
	/// This command requires that you have a local user in the room with at
	/// least a moderator power level. It will first attempt to raise power
	/// levels so that nobody can use the room further, then remove the
	/// canonical alias event, sets the history visibility to `joined`,
	/// sets the join rules to `org.continuwuity.shutdown` (preventing anyone
	/// from joining even with an invite), and then bans or kicks all users,
	/// setting the MSC4293 "redact events" flag on those users if possible.
	/// Finally, it will send a room tombstone event, which will effectively
	/// make the room unusable on most clients even if the room state resets.
	///
	/// This effectively will make the room unusable, unjoinable, and removes
	/// everyone from it. This is as close to a "shutdown" as you can get with
	/// federation.
	ShutdownRoom {
		/// If no local users with a power level are joined to the room, setting
		/// this flag will attempt one, and will join the user with the
		/// highest power level to the room to perform the shutdown.
		///
		/// If this flag is not set, and no local users can perform the
		/// shutdown, no further attempt will be made.
		#[arg(long)]
		force: bool,
		/// Whether to use MSC4293 fields to indicate that all messages in the
		/// room should be redacted. This will make it more difficult for
		/// clients that implement MSC4293 (like Element) to render the room
		/// in the event users manage to rejoin.
		#[arg(long)]
		redact: bool,

		///
		#[arg(long)]
		yes_i_am_sure_i_want_to_irreversibly_shutdown_this_room_destroying_it_in_the_process:
			bool,

		/// The room in the format of `!roomid:example.com` or a room alias in
		/// the format of `#roomalias:example.com`
		room: OwnedRoomOrAliasId,
	},
}

#[admin_command]
async fn ban_room(&self, room: OwnedRoomOrAliasId) -> Result {
	debug!("Got room alias or ID: {}", room);

	let admin_room_alias = &self.services.globals.admin_alias;

	if let Ok(admin_room_id) = self.services.admin.get_admin_room().await {
		if room.to_string().eq(&admin_room_id) || room.to_string().eq(admin_room_alias) {
			return Err!("Not allowed to ban the admin room.");
		}
	}

	let room_id = if room.is_room_id() {
		let room_id = match RoomId::parse(&room) {
			| Ok(room_id) => room_id,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		debug!("Room specified is a room ID, banning room ID");

		room_id.to_owned()
	} else if room.is_room_alias_id() {
		let room_alias = match RoomAliasId::parse(&room) {
			| Ok(room_alias) => room_alias,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		debug!(
			"Room specified is not a room ID, attempting to resolve room alias to a room ID \
			 locally, if not using get_alias_helper to fetch room ID remotely"
		);

		match self
			.services
			.rooms
			.alias
			.resolve_alias(room_alias, None)
			.await
		{
			| Ok((room_id, servers)) => {
				debug!(
					%room_id,
					?servers,
					"Got federation response fetching room ID for room {room}"
				);
				room_id
			},
			| Err(e) => {
				return Err!("Failed to resolve room alias {room} to a room ID: {e}");
			},
		}
	} else {
		return Err!(
			"Room specified is not a room ID or room alias. Please note that this requires a \
			 full room ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
			 (`#roomalias:example.com`)",
		);
	};

	info!("Making all users leave the room {room_id} and forgetting it");
	let mut users = self
		.services
		.rooms
		.state_cache
		.room_members(&room_id)
		.map(ToOwned::to_owned)
		.ready_filter(|user| self.services.globals.user_is_local(user))
		.boxed();

	while let Some(ref user_id) = users.next().await {
		info!(
			"Attempting leave for user {user_id} in room {room_id} (ignoring all errors, \
			 evicting admins too)",
		);

		if let Err(e) = leave_room(self.services, user_id, &room_id, None)
			.boxed()
			.await
		{
			warn!("Failed to leave room: {e}");
		}

		self.services.rooms.state_cache.forget(&room_id, user_id);
	}

	self.services
		.rooms
		.alias
		.local_aliases_for_room(&room_id)
		.map(ToOwned::to_owned)
		.for_each(|local_alias| async move {
			self.services
				.rooms
				.alias
				.remove_alias(&local_alias, &self.services.globals.server_user)
				.await
				.ok();
		})
		.await;

	self.services.rooms.directory.set_not_public(&room_id); // remove from the room directory
	self.services.rooms.metadata.ban_room(&room_id, true); // prevent further joins
	self.services.rooms.metadata.disable_room(&room_id, true); // disable federation

	self.write_str(
		"Room banned, removed all our local users, and disabled incoming federation with room.",
	)
	.await
}

#[admin_command]
async fn ban_list_of_rooms(&self) -> Result {
	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&"").trim() != "```"
	{
		return Err!("Expected code block in command body. Add --help for details.",);
	}

	let rooms_s = self
		.body
		.to_vec()
		.drain(1..self.body.len().saturating_sub(1))
		.collect::<Vec<_>>();

	let admin_room_alias = &self.services.globals.admin_alias;

	let mut room_ban_count: usize = 0;
	let mut room_ids: Vec<OwnedRoomId> = Vec::new();

	for &room in &rooms_s {
		match <&RoomOrAliasId>::try_from(room) {
			| Ok(room_alias_or_id) => {
				if let Ok(admin_room_id) = self.services.admin.get_admin_room().await {
					if room.to_owned().eq(&admin_room_id) || room.to_owned().eq(admin_room_alias)
					{
						warn!("User specified admin room in bulk ban list, ignoring");
						continue;
					}
				}

				if room_alias_or_id.is_room_id() {
					let room_id = match RoomId::parse(room_alias_or_id) {
						| Ok(room_id) => room_id,
						| Err(e) => {
							// ignore rooms we failed to parse
							warn!(
								"Error parsing room \"{room}\" during bulk room banning, \
								 ignoring error and logging here: {e}"
							);
							continue;
						},
					};

					room_ids.push(room_id.to_owned());
				}

				if room_alias_or_id.is_room_alias_id() {
					match RoomAliasId::parse(room_alias_or_id) {
						| Ok(room_alias) => {
							let room_id = match self
								.services
								.rooms
								.alias
								.resolve_local_alias(room_alias)
								.await
							{
								| Ok(room_id) => room_id,
								| _ => {
									debug!(
										"We don't have this room alias to a room ID locally, \
										 attempting to fetch room ID over federation"
									);

									match self
										.services
										.rooms
										.alias
										.resolve_alias(room_alias, None)
										.await
									{
										| Ok((room_id, servers)) => {
											debug!(
												%room_id,
												?servers,
												"Got federation response fetching room ID for \
												 {room}",
											);
											room_id
										},
										| Err(e) => {
											warn!(
												"Failed to resolve room alias {room} to a room \
												 ID: {e}"
											);
											continue;
										},
									}
								},
							};

							room_ids.push(room_id);
						},
						| Err(e) => {
							warn!(
								"Error parsing room \"{room}\" during bulk room banning, \
								 ignoring error and logging here: {e}"
							);
							continue;
						},
					}
				}
			},
			| Err(e) => {
				warn!(
					"Error parsing room \"{room}\" during bulk room banning, ignoring error and \
					 logging here: {e}"
				);
				continue;
			},
		}
	}

	for room_id in room_ids {
		debug!("Banned {room_id} successfully");
		room_ban_count = room_ban_count.saturating_add(1);

		debug!("Making all users leave the room {room_id} and forgetting it");
		let mut users = self
			.services
			.rooms
			.state_cache
			.room_members(&room_id)
			.map(ToOwned::to_owned)
			.ready_filter(|user| self.services.globals.user_is_local(user))
			.boxed();

		while let Some(ref user_id) = users.next().await {
			debug!(
				"Attempting leave for user {user_id} in room {room_id} (ignoring all errors, \
				 evicting admins too)",
			);

			if let Err(e) = leave_room(self.services, user_id, &room_id, None)
				.boxed()
				.await
			{
				warn!("Failed to leave room: {e}");
			}

			self.services.rooms.state_cache.forget(&room_id, user_id);
		}

		// remove any local aliases, ignore errors
		self.services
			.rooms
			.alias
			.local_aliases_for_room(&room_id)
			.map(ToOwned::to_owned)
			.for_each(|local_alias| async move {
				self.services
					.rooms
					.alias
					.remove_alias(&local_alias, &self.services.globals.server_user)
					.await
					.ok();
			})
			.await;

		self.services.rooms.metadata.ban_room(&room_id, true);
		// unpublish from room directory, ignore errors
		self.services.rooms.directory.set_not_public(&room_id);
		self.services.rooms.metadata.disable_room(&room_id, true);
	}

	self.write_str(&format!(
		"Finished bulk room ban, banned {room_ban_count} total rooms, evicted all users, and \
		 disabled incoming federation with the room."
	))
	.await
}

#[admin_command]
async fn unban_room(&self, room: OwnedRoomOrAliasId) -> Result {
	let room_id = if room.is_room_id() {
		let room_id = match RoomId::parse(&room) {
			| Ok(room_id) => room_id,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		debug!("Room specified is a room ID, unbanning room ID");
		self.services.rooms.metadata.ban_room(room_id, false);

		room_id.to_owned()
	} else if room.is_room_alias_id() {
		let room_alias = match RoomAliasId::parse(&room) {
			| Ok(room_alias) => room_alias,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		debug!(
			"Room specified is not a room ID, attempting to resolve room alias to a room ID \
			 locally, if not using get_alias_helper to fetch room ID remotely"
		);

		let room_id = match self
			.services
			.rooms
			.alias
			.resolve_local_alias(room_alias)
			.await
		{
			| Ok(room_id) => room_id,
			| _ => {
				debug!(
					"We don't have this room alias to a room ID locally, attempting to fetch \
					 room ID over federation"
				);

				match self
					.services
					.rooms
					.alias
					.resolve_alias(room_alias, None)
					.await
				{
					| Ok((room_id, servers)) => {
						debug!(
							%room_id,
							?servers,
							"Got federation response fetching room ID for room {room}"
						);
						room_id
					},
					| Err(e) => {
						return Err!("Failed to resolve room alias {room} to a room ID: {e}");
					},
				}
			},
		};

		self.services.rooms.metadata.ban_room(&room_id, false);

		room_id
	} else {
		return Err!(
			"Room specified is not a room ID or room alias. Please note that this requires a \
			 full room ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
			 (`#roomalias:example.com`)",
		);
	};

	self.services.rooms.metadata.disable_room(&room_id, false);
	self.write_str("Room unbanned and federation re-enabled.")
		.await
}

#[admin_command]
async fn list_banned_rooms(&self, no_details: bool) -> Result {
	let room_ids: Vec<OwnedRoomId> = self
		.services
		.rooms
		.metadata
		.list_banned_rooms()
		.map(Into::into)
		.collect()
		.await;

	if room_ids.is_empty() {
		return Err!("No rooms are banned.");
	}

	let mut rooms = room_ids
		.iter()
		.stream()
		.then(|room_id| get_room_info(self.services, room_id))
		.collect::<Vec<_>>()
		.await;

	rooms.sort_by_key(|r| r.1);
	rooms.reverse();

	let num = rooms.len();

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

	self.write_str(&format!("Rooms Banned ({num}):\n```\n{body}\n```",))
		.await
}

#[admin_command]
async fn takeover(&self, force: bool, room: OwnedRoomOrAliasId) -> Result {
	let room_id = if room.is_room_id() {
		let room_id = match RoomId::parse(&room) {
			| Ok(room_id) => room_id,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		room_id.to_owned()
	} else if room.is_room_alias_id() {
		let room_alias = match RoomAliasId::parse(&room) {
			| Ok(room_alias) => room_alias,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		match self
			.services
			.rooms
			.alias
			.resolve_alias(room_alias, None)
			.await
		{
			| Ok((room_id, servers)) => {
				debug!(
					?room_id,
					?servers,
					"Got federation response fetching room ID for room {room}"
				);
				room_id
			},
			| Err(e) => {
				return Err!("Failed to resolve room alias {room} to a room ID: {e}");
			},
		}
	} else {
		return Err!(
			"Room specified is not a room ID or room alias. Please note that this requires a \
			 full room ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
			 (`#roomalias:example.com`)",
		);
	};

	let room_version =
		RoomVersion::new(&self.services.rooms.state.get_room_version(&room_id).await?)?;
	let Ok(create_content) = self
		.services
		.rooms
		.state_accessor
		.room_state_get_content::<RoomCreateEventContent>(
			&room_id,
			&StateEventType::RoomCreate,
			"",
		)
		.await
	else {
		return Err!("Failed to get room create event");
	};
	let mut power_levels = match self
		.services
		.rooms
		.state_accessor
		.room_state_get_content::<RoomPowerLevelsEventContent>(
			&room_id,
			&StateEventType::RoomPowerLevels,
			"",
		)
		.await
	{
		| Ok(content) => content,
		| Err(e) => {
			return Err!("Failed to get power levels for room {room_id}: {e}");
		},
	};
	let local_creators = if room_version.explicitly_privilege_room_creators
		&& create_content.additional_creators.is_some()
	{
		create_content
			.additional_creators
			.clone()
			.unwrap()
			.into_iter()
			.filter(|user_id| self.services.globals.user_is_local(user_id))
			.collect::<Vec<_>>()
	} else {
		vec![]
	};
	let local_users = power_levels
		.users
		.iter()
		.filter(|(user_id, _)| self.services.globals.user_is_local(user_id))
		.map(|(user_id, level)| (user_id.clone(), *level))
		.collect::<BTreeMap<_, _>>();
	let min_pl = power_levels
		.events
		.get(&StateEventType::RoomPowerLevels.into())
		.copied()
		.unwrap_or(power_levels.state_default);
	let mut ordered_users = local_users
		.iter()
		.map(|(user_id, level)| {
			if local_creators.contains(user_id) {
				(user_id, Int::MAX)
			} else {
				(user_id, *level)
			}
		})
		.filter(|(user_id, level)| *level >= min_pl || local_creators.contains(*user_id))
		.collect::<Vec<_>>();
	ordered_users.sort_by_key(|(_, level)| level.saturating_mul(Int::from(-1)));

	for (user_id, powerlevel) in ordered_users {
		if !self
			.services
			.rooms
			.state_cache
			.is_joined(user_id.as_ref(), &room_id)
			.await
		{
			if !force {
				continue;
			}
			info!("Joining {user_id} to room {room_id} to perform takeover");
			let lock = self.services.rooms.state.mutex.lock(&room_id).await;
			if let Err(e) = self
				.services
				.rooms
				.timeline
				.build_and_append_pdu(
					conduwuit::pdu::Builder::state(
						String::from(user_id.as_str()),
						&RoomMemberEventContent::new(MembershipState::Join),
					),
					user_id,
					Some(&room_id),
					&lock,
				)
				.await
			{
				warn!("Failed to join {user_id} to room {room_id} to perform takeover: {e}");
				drop(lock);
				continue;
			}
			drop(lock);
		}
		info!("Promoting you to power level {powerlevel} in room {room_id} via {user_id}");
		let lock = self.services.rooms.state.mutex.lock(&room_id).await;
		power_levels
			.users
			.insert(self.sender.expect("you should exist").to_owned(), powerlevel);
		if let Err(e) = self
			.services
			.rooms
			.timeline
			.build_and_append_pdu(
				conduwuit::pdu::Builder::state(String::new(), &power_levels),
				user_id,
				Some(&room_id),
				&lock,
			)
			.await
		{
			warn!(
				"Failed to promote you to power level {powerlevel} in room {room_id} via \
				 {user_id}: {e}"
			);
			drop(lock);
			continue;
		}
		return self
			.write_str(&format!(
				"Successfully promoted you to power level {powerlevel} in room {room_id} via \
				 {user_id}"
			))
			.await;
	}

	self.write_str("Failed to promote you, no local users with sufficient power level found.")
		.await
}

#[admin_command]
async fn shutdown_room(
	&self,
	force: bool,
	redact: bool,
	yes_i_am_sure_i_want_to_irreversibly_shutdown_this_room_destroying_it_in_the_process: bool,
	room: OwnedRoomOrAliasId,
) -> Result {
	let room_id = if room.is_room_id() {
		let room_id = match RoomId::parse(&room) {
			| Ok(room_id) => room_id,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		room_id.to_owned()
	} else if room.is_room_alias_id() {
		let room_alias = match RoomAliasId::parse(&room) {
			| Ok(room_alias) => room_alias,
			| Err(e) => {
				return Err!(
					"Failed to parse room ID {room}. Please note that this requires a full room \
					 ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
					 (`#roomalias:example.com`): {e}"
				);
			},
		};

		match self
			.services
			.rooms
			.alias
			.resolve_alias(room_alias, None)
			.await
		{
			| Ok((room_id, servers)) => {
				debug!(
					?room_id,
					?servers,
					"Got federation response fetching room ID for room {room}"
				);
				room_id
			},
			| Err(e) => {
				return Err!("Failed to resolve room alias {room} to a room ID: {e}");
			},
		}
	} else {
		return Err!(
			"Room specified is not a room ID or room alias. Please note that this requires a \
			 full room ID (`!awIh6gGInaS5wLQJwa:example.com`) or a room alias \
			 (`#roomalias:example.com`)",
		);
	};

	if !yes_i_am_sure_i_want_to_irreversibly_shutdown_this_room_destroying_it_in_the_process {
		return Err!(
			"This command is irreversible and will immediately shutdown the room, making it \
			 completely unusable if successful. If you are sure you want to do this, add the \
			 flag --yes-i-am-sure-i-want-to-irreversibly-shutdown-this-room-destroying-it-in-the-process \
			 to your command."
		);
	}

	let mut power_levels: RoomPowerLevelsEventContent = match self
		.services
		.rooms
		.state_accessor
		.room_state_get_content(&room_id, &StateEventType::RoomPowerLevels, "")
		.await
		.map_err(|e| Err!("Failed to get power levels for room {room_id}: {e}"))
	{
		| Ok(content) => content,
		| Err(e) => {
			return e;
		},
	};

	let mut joined_users = self
		.services
		.rooms
		.state_cache
		.room_members(&room_id)
		.map(ToOwned::to_owned)
		.collect::<Vec<_>>()
		.await;

	let room_version =
		RoomVersion::new(&self.services.rooms.state.get_room_version(&room_id).await?)?;
	let Ok(create_content) = self
		.services
		.rooms
		.state_accessor
		.room_state_get_content::<RoomCreateEventContent>(
			&room_id,
			&StateEventType::RoomCreate,
			"",
		)
		.await
	else {
		return Err!("Failed to get room create event");
	};
	let local_creators = if room_version.explicitly_privilege_room_creators
		&& create_content.additional_creators.is_some()
	{
		create_content
			.additional_creators
			.unwrap()
			.into_iter()
			.filter(|user_id| self.services.globals.user_is_local(user_id))
			.collect::<Vec<_>>()
	} else {
		vec![]
	};
	let local_users = power_levels
		.users
		.iter()
		.filter(|(user_id, _)| self.services.globals.user_is_local(user_id))
		.map(|(user_id, level)| (user_id.clone(), *level))
		.collect::<BTreeMap<_, _>>();
	let join_rules_pl = power_levels
		.events
		.get(&StateEventType::RoomJoinRules.into())
		.copied()
		.unwrap_or(power_levels.state_default);
	let kick_pl = power_levels.kick;
	let ban_pl = power_levels.ban;
	let min_pl = join_rules_pl.min(kick_pl).min(ban_pl);
	let mut ordered_users = local_users
		.iter()
		.map(|(user_id, level)| {
			if local_creators.contains(user_id) {
				(user_id, Int::MAX)
			} else {
				(user_id, *level)
			}
		})
		.filter(|(user_id, level)| *level >= min_pl || local_creators.contains(*user_id))
		.collect::<Vec<_>>();
	ordered_users.sort_by_key(|(_, level)| level.saturating_mul(Int::from(-1)));

	let mut changed_join_rules = false;
	let mut changed_history_visibility = false;
	let mut changed_power_levels = false;
	let mut sent_tombstone = false;
	let mut removed_ok: u32 = 0;

	for (user_id, powerlevel) in ordered_users {
		let new_membership = if powerlevel >= ban_pl {
			MembershipState::Ban
		} else {
			MembershipState::Leave
		};
		if !self
			.services
			.rooms
			.state_cache
			.is_joined(user_id.as_ref(), &room_id)
			.await
		{
			if !force {
				continue;
			}
			info!("Joining {user_id} to room {room_id} to perform shutdown");
			let lock = self.services.rooms.state.mutex.lock(&room_id).await;
			if let Err(e) = self
				.services
				.rooms
				.timeline
				.build_and_append_pdu(
					conduwuit::pdu::Builder::state(
						String::from(user_id.as_str()),
						&RoomMemberEventContent::new(MembershipState::Join),
					),
					user_id,
					Some(&room_id),
					&lock,
				)
				.await
			{
				warn!("Failed to join {user_id} to room {room_id} to perform shutdown: {e}");
				drop(lock);
				continue;
			}
			drop(lock);
		}
		if !changed_power_levels {
			info!("Raising minimum power levels to {powerlevel} via {user_id}");
			power_levels.events_default = power_levels.events_default.max(powerlevel);
			power_levels.state_default = power_levels.state_default.max(powerlevel);
			if power_levels.users_default < powerlevel {
				power_levels.users_default = Int::MIN;
			}
			power_levels.kick = power_levels.kick.max(powerlevel);
			power_levels.ban = power_levels.ban.max(powerlevel);
			for (event_type, event_pl) in power_levels.events.clone() {
				power_levels
					.events
					.insert(event_type, event_pl.max(powerlevel));
			}
			for (user, user_pl) in power_levels.users.clone() {
				if user_pl < powerlevel {
					power_levels.users.remove(&user);
				}
			}
			let lock = self.services.rooms.state.mutex.lock(&room_id).await;
			if let Err(e) = self
				.services
				.rooms
				.timeline
				.build_and_append_pdu(
					conduwuit::pdu::Builder::state(String::new(), &power_levels.clone()),
					user_id,
					Some(&room_id),
					&lock,
				)
				.await
			{
				warn!(
					"Failed to raise power levels to {powerlevel} in room {room_id} via \
					 {user_id}: {e}"
				);
			} else {
				changed_power_levels = true;
			}
			drop(lock);
		}
		if !changed_join_rules {
			info!("Setting room to private via {user_id}");
			// NOTE: Setting the room to `private` soft-bricks it, as new joins with this
			// join rule can actually be authorised.
			let lock = self.services.rooms.state.mutex.lock(&room_id).await;
			if let Err(e) = self
				.services
				.rooms
				.timeline
				.build_and_append_pdu(
					conduwuit::pdu::Builder::state(
						String::new(),
						&RoomJoinRulesEventContent::new(
							JoinRule::deserialize(json!("\"org.continuwuity.shutdown\""))
								.expect("valid fixed json"),
						),
					),
					user_id,
					Some(&room_id),
					&lock,
				)
				.await
			{
				warn!("Failed to set room to private in room {room_id} via {user_id}: {e}");
			} else {
				changed_join_rules = true;
			}
			drop(lock);
		}
		if !changed_history_visibility {
			info!("Setting history visibility to joined via {user_id}");
			let lock = self.services.rooms.state.mutex.lock(&room_id).await;
			if let Err(e) = self
				.services
				.rooms
				.timeline
				.build_and_append_pdu(
					conduwuit::pdu::Builder::state(
						String::new(),
						&RoomHistoryVisibilityEventContent::new(HistoryVisibility::Joined),
					),
					user_id,
					Some(&room_id),
					&lock,
				)
				.await
			{
				warn!(
					"Failed to set history visibility to joined in room {room_id} via \
					 {user_id}: {e}"
				);
			} else {
				changed_history_visibility = true;
			}
			drop(lock);
		}
		info!("Removing {} users in {room_id} via {user_id}", joined_users.len());
		let lock = self.services.rooms.state.mutex.lock(&room_id).await;
		for remove_user in &joined_users.clone() {
			if remove_user == user_id || self.services.admin.user_is_admin(user_id).await {
				continue;
			}
			debug!("Removing {remove_user} via {user_id}");
			if let Err(e) = self
				.services
				.rooms
				.timeline
				.build_and_append_pdu(
					conduwuit::pdu::Builder::state(
						String::from(remove_user.as_str()),
						&RoomMemberEventContent {
							membership: new_membership.clone(),
							redact_events: if redact { Some(true) } else { None },
							..RoomMemberEventContent::new(new_membership.clone())
						},
					),
					user_id,
					Some(&room_id),
					&lock,
				)
				.await
			{
				warn!("Failed to remove {remove_user} via {user_id}: {e}");
				continue;
			}
			removed_ok = removed_ok.saturating_add(1);
			if self.services.globals.user_is_local(remove_user) {
				self.services
					.rooms
					.state_cache
					.forget(&room_id, remove_user);
			}
			joined_users.retain(|u| u != remove_user);
		}
		if !sent_tombstone {
			info!("Sending tombstone event for {room_id} via {user_id}");
			if let Err(e) = self
				.services
				.rooms
				.timeline
				.build_and_append_pdu(
					conduwuit::pdu::Builder::state(
						String::new(),
						&RoomTombstoneEventContent::new(
							format!("Room {room_id} has been shut down"),
							room_id.clone(),
						),
					),
					user_id,
					Some(&room_id),
					&lock,
				)
				.await
			{
				warn!("Failed to send tombstone event for {room_id} via {user_id}: {e}");
			} else {
				sent_tombstone = true;
			}
		}
	}
	self.write_str(&format!(
		"Room shutdown complete, removed {removed_ok} users, changed join rules: \
		 {changed_join_rules}.\nConsider banning the room with `ban-room`.",
	))
	.await
}
