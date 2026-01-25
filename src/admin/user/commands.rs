use std::{
	collections::{BTreeMap, HashSet},
	fmt::Write as _,
};

use api::client::{full_user_deactivate, join_room_by_id_helper, leave_room, remote_leave_room};
use conduwuit::{
	Err, Result, debug, debug_warn, error, info, is_equal_to,
	matrix::{Event, pdu::PduBuilder},
	utils::{self, ReadyExt},
	warn,
};
use futures::{FutureExt, StreamExt};
use ruma::{
	OwnedEventId, OwnedRoomId, OwnedRoomOrAliasId, OwnedServerName, OwnedUserId, UserId,
	events::{
		RoomAccountDataEventType, StateEventType,
		room::{
			power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent},
			redaction::RoomRedactionEventContent,
		},
		tag::{TagEvent, TagEventContent, TagInfo},
	},
};

use crate::{
	admin_command, get_room_info,
	utils::{parse_active_local_user_id, parse_local_user_id},
};

const AUTO_GEN_PASSWORD_LENGTH: usize = 25;
const BULK_JOIN_REASON: &str = "Bulk force joining this room as initiated by the server admin.";

#[admin_command]
pub(super) async fn list_users(&self) -> Result {
	let users: Vec<_> = self
		.services
		.users
		.list_local_users()
		.map(ToString::to_string)
		.collect()
		.await;

	let mut plain_msg = format!("Found {} local user account(s):\n```\n", users.len());
	plain_msg += users.join("\n").as_str();
	plain_msg += "\n```";

	self.write_str(&plain_msg).await
}

#[admin_command]
pub(super) async fn create_user(&self, username: String, password: Option<String>) -> Result {
	// Validate user id
	let user_id = parse_local_user_id(self.services, &username)?;

	if let Err(e) = user_id.validate_strict() {
		if self.services.config.emergency_password.is_none() {
			return Err!("Username {user_id} contains disallowed characters or spaces: {e}");
		}
	}

	if self.services.users.exists(&user_id).await {
		return Err!("User {user_id} already exists");
	}

	let password = password.unwrap_or_else(|| utils::random_string(AUTO_GEN_PASSWORD_LENGTH));

	// Create user
	self.services
		.users
		.create(&user_id, Some(password.as_str()), None)
		.await?;

	// Default to pretty displayname
	let mut displayname = user_id.localpart().to_owned();

	// If `new_user_displayname_suffix` is set, registration will push whatever
	// content is set to the user's display name with a space before it
	if !self
		.services
		.server
		.config
		.new_user_displayname_suffix
		.is_empty()
	{
		write!(displayname, " {}", self.services.server.config.new_user_displayname_suffix)?;
	}

	self.services
		.users
		.set_displayname(&user_id, Some(displayname));

	// Initial account data
	self.services
		.account_data
		.update(
			None,
			&user_id,
			ruma::events::GlobalAccountDataEventType::PushRules
				.to_string()
				.into(),
			&serde_json::to_value(ruma::events::push_rules::PushRulesEvent {
				content: ruma::events::push_rules::PushRulesEventContent {
					global: ruma::push::Ruleset::server_default(&user_id),
				},
			})?,
		)
		.await?;

	if !self.services.server.config.auto_join_rooms.is_empty() {
		for room in &self.services.server.config.auto_join_rooms {
			let Ok(room_id) = self.services.rooms.alias.resolve(room).await else {
				error!(
					%user_id,
					"Failed to resolve room alias to room ID when attempting to auto join {room}, skipping"
				);
				continue;
			};

			if !self
				.services
				.rooms
				.state_cache
				.server_in_room(self.services.globals.server_name(), &room_id)
				.await
			{
				warn!(
					"Skipping room {room} to automatically join as we have never joined before."
				);
				continue;
			}

			if let Some(room_server_name) = room.server_name() {
				match join_room_by_id_helper(
					self.services,
					&user_id,
					&room_id,
					Some("Automatically joining this room upon registration".to_owned()),
					&[
						self.services.globals.server_name().to_owned(),
						room_server_name.to_owned(),
					],
					None,
					&None,
				)
				.await
				{
					| Ok(_response) => {
						info!("Automatically joined room {room} for user {user_id}");
					},
					| Err(e) => {
						// don't return this error so we don't fail registrations
						error!(
							"Failed to automatically join room {room} for user {user_id}: {e}"
						);
						self.services
							.admin
							.send_text(&format!(
								"Failed to automatically join room {room} for user {user_id}: \
								 {e}"
							))
							.await;
					},
				}
			}
		}
	}

	// we dont add a device since we're not the user, just the creator

	// if this account creation is from the CLI / --execute, invite the first user
	// to admin room
	if let Ok(admin_room) = self.services.admin.get_admin_room().await {
		if self
			.services
			.rooms
			.state_cache
			.room_joined_count(&admin_room)
			.await
			.is_ok_and(is_equal_to!(1))
		{
			self.services
				.admin
				.make_user_admin(&user_id)
				.boxed()
				.await?;
			warn!("Granting {user_id} admin privileges as the first user");
		}
	} else {
		debug!("create_user admin command called without an admin room being available");
	}

	self.write_str(&format!("Created user with user_id: {user_id} and password: `{password}`"))
		.await
}

#[admin_command]
pub(super) async fn deactivate(&self, no_leave_rooms: bool, user_id: String) -> Result {
	// Validate user id
	let user_id = parse_local_user_id(self.services, &user_id)?;

	// don't deactivate the server service account
	if user_id == self.services.globals.server_user {
		return Err!("Not allowed to deactivate the server service account.",);
	}

	self.services.users.deactivate_account(&user_id).await?;

	if !no_leave_rooms {
		self.services
			.admin
			.send_text(&format!("Making {user_id} leave all rooms after deactivation..."))
			.await;

		let all_joined_rooms: Vec<OwnedRoomId> = self
			.services
			.rooms
			.state_cache
			.rooms_joined(&user_id)
			.map(Into::into)
			.collect()
			.await;

		full_user_deactivate(self.services, &user_id, &all_joined_rooms)
			.boxed()
			.await?;
	}

	self.write_str(&format!("User {user_id} has been deactivated"))
		.await
}

#[admin_command]
pub(super) async fn suspend(&self, user_id: String) -> Result {
	self.bail_restricted()?;
	let user_id = parse_local_user_id(self.services, &user_id)?;

	if user_id == self.services.globals.server_user {
		return Err!("Not allowed to suspend the server service account.",);
	}

	if !self.services.users.exists(&user_id).await {
		return Err!("User {user_id} does not exist.");
	}
	if self.services.users.is_admin(&user_id).await {
		return Err!("Admin users cannot be suspended.");
	}
	// TODO: Record the actual user that sent the suspension where possible
	self.services
		.users
		.suspend_account(&user_id, self.sender_or_service_user())
		.await;

	self.write_str(&format!("User {user_id} has been suspended."))
		.await
}

#[admin_command]
pub(super) async fn unsuspend(&self, user_id: String) -> Result {
	self.bail_restricted()?;
	let user_id = parse_local_user_id(self.services, &user_id)?;

	if user_id == self.services.globals.server_user {
		return Err!("Not allowed to unsuspend the server service account.",);
	}

	if !self.services.users.exists(&user_id).await {
		return Err!("User {user_id} does not exist.");
	}
	self.services.users.unsuspend_account(&user_id).await;

	self.write_str(&format!("User {user_id} has been unsuspended."))
		.await
}

#[admin_command]
pub(super) async fn reset_password(
	&self,
	logout: bool,
	username: String,
	password: Option<String>,
) -> Result {
	let user_id = parse_local_user_id(self.services, &username)?;

	if user_id == self.services.globals.server_user {
		return Err!(
			"Not allowed to set the password for the server account. Please use the emergency \
			 password config option.",
		);
	}

	let new_password = password.unwrap_or_else(|| utils::random_string(AUTO_GEN_PASSWORD_LENGTH));

	match self
		.services
		.users
		.set_password(&user_id, Some(new_password.as_str()))
		.await
	{
		| Err(e) => return Err!("Couldn't reset the password for user {user_id}: {e}"),
		| Ok(()) => {
			write!(self, "Successfully reset the password for user {user_id}: `{new_password}`")
		},
	}
	.await?;

	if logout {
		self.services
			.users
			.all_device_ids(&user_id)
			.for_each(|device_id| self.services.users.remove_device(&user_id, device_id))
			.await;
		write!(self, "\nAll existing sessions have been logged out.").await?;
	}

	Ok(())
}

#[admin_command]
pub(super) async fn deactivate_all(&self, no_leave_rooms: bool, force: bool) -> Result {
	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&"").trim() != "```"
	{
		return Err!("Expected code block in command body. Add --help for details.",);
	}

	let usernames = self
		.body
		.to_vec()
		.drain(1..self.body.len().saturating_sub(1))
		.collect::<Vec<_>>();

	let mut user_ids: Vec<OwnedUserId> = Vec::with_capacity(usernames.len());
	let mut admins = Vec::new();

	for username in usernames {
		match parse_active_local_user_id(self.services, username).await {
			| Err(e) => {
				self.services
					.admin
					.send_text(&format!("{username} is not a valid username, skipping over: {e}"))
					.await;

				continue;
			},
			| Ok(user_id) => {
				if self.services.users.is_admin(&user_id).await && !force {
					self.services
						.admin
						.send_text(&format!(
							"{username} is an admin and --force is not set, skipping over"
						))
						.await;

					admins.push(username);
					continue;
				}

				// don't deactivate the server service account
				if user_id == self.services.globals.server_user {
					self.services
						.admin
						.send_text(&format!(
							"{username} is the server service account, skipping over"
						))
						.await;

					continue;
				}

				user_ids.push(user_id);
			},
		}
	}

	let mut deactivation_count: usize = 0;

	for user_id in user_ids {
		match self.services.users.deactivate_account(&user_id).await {
			| Err(e) => {
				self.services
					.admin
					.send_text(&format!("Failed deactivating user: {e}"))
					.await;
			},
			| Ok(()) => {
				deactivation_count = deactivation_count.saturating_add(1);
				if !no_leave_rooms {
					info!("Forcing user {user_id} to leave all rooms apart of deactivate-all");
					let all_joined_rooms: Vec<OwnedRoomId> = self
						.services
						.rooms
						.state_cache
						.rooms_joined(&user_id)
						.map(Into::into)
						.collect()
						.await;

					full_user_deactivate(self.services, &user_id, &all_joined_rooms)
						.boxed()
						.await?;
				}
			},
		}
	}

	if admins.is_empty() {
		write!(self, "Deactivated {deactivation_count} accounts.")
	} else {
		write!(
			self,
			"Deactivated {deactivation_count} accounts.\nSkipped admin accounts: {}. Use \
			 --force to deactivate admin accounts",
			admins.join(", ")
		)
	}
	.await
}

#[admin_command]
pub(super) async fn list_joined_rooms(&self, user_id: String) -> Result {
	// Validate user id
	let user_id = parse_local_user_id(self.services, &user_id)?;

	let mut rooms: Vec<(OwnedRoomId, u64, String)> = self
		.services
		.rooms
		.state_cache
		.rooms_joined(&user_id)
		.then(|room_id| get_room_info(self.services, room_id))
		.collect()
		.await;

	if rooms.is_empty() {
		return Err!("User is not in any rooms.");
	}

	rooms.sort_by_key(|r| r.1);
	rooms.reverse();

	let body = rooms
		.iter()
		.map(|(id, members, name)| format!("{id}\tMembers: {members}\tName: {name}"))
		.collect::<Vec<_>>()
		.join("\n");

	self.write_str(&format!("Rooms {user_id} Joined ({}):\n```\n{body}\n```", rooms.len(),))
		.await
}

#[admin_command]
pub(super) async fn force_join_list_of_local_users(
	&self,
	room_id: OwnedRoomOrAliasId,
	yes_i_want_to_do_this: bool,
) -> Result {
	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&"").trim() != "```"
	{
		return Err!("Expected code block in command body. Add --help for details.",);
	}

	if !yes_i_want_to_do_this {
		return Err!(
			"You must pass the --yes-i-want-to-do-this-flag to ensure you really want to force \
			 bulk join all specified local users.",
		);
	}

	let server_admins = self.services.admin.get_admins().await;

	if server_admins.is_empty() {
		return Err!("There are no admins set for this server.");
	}

	let (room_id, servers) = self
		.services
		.rooms
		.alias
		.resolve_with_servers(&room_id, None)
		.await?;

	if !self
		.services
		.rooms
		.state_cache
		.server_in_room(self.services.globals.server_name(), &room_id)
		.await
	{
		return Err!("We are not joined in this room.");
	}

	if !self
		.services
		.rooms
		.state_cache
		.room_members(&room_id)
		.ready_any(|user_id| server_admins.contains(&user_id.to_owned()))
		.await
	{
		return Err!("There is not a single server admin in the room.",);
	}

	let usernames = self
		.body
		.to_vec()
		.drain(1..self.body.len().saturating_sub(1))
		.collect::<Vec<_>>();

	let mut user_ids: Vec<OwnedUserId> = Vec::with_capacity(usernames.len());

	for username in usernames {
		match parse_active_local_user_id(self.services, username).await {
			| Ok(user_id) => {
				// don't make the server service account join
				if user_id == self.services.globals.server_user {
					self.services
						.admin
						.send_text(&format!(
							"{username} is the server service account, skipping over"
						))
						.await;

					continue;
				}

				user_ids.push(user_id);
			},
			| Err(e) => {
				self.services
					.admin
					.send_text(&format!("{username} is not a valid username, skipping over: {e}"))
					.await;

				continue;
			},
		}
	}

	let mut failed_joins: usize = 0;
	let mut successful_joins: usize = 0;

	for user_id in user_ids {
		match join_room_by_id_helper(
			self.services,
			&user_id,
			&room_id,
			Some(String::from(BULK_JOIN_REASON)),
			&servers,
			None,
			&None,
		)
		.await
		{
			| Ok(_res) => {
				successful_joins = successful_joins.saturating_add(1);
			},
			| Err(e) => {
				debug_warn!("Failed force joining {user_id} to {room_id} during bulk join: {e}");
				failed_joins = failed_joins.saturating_add(1);
			},
		}
	}

	self.write_str(&format!(
		"{successful_joins} local users have been joined to {room_id}. {failed_joins} joins \
		 failed.",
	))
	.await
}

#[admin_command]
pub(super) async fn force_join_all_local_users(
	&self,
	room_id: OwnedRoomOrAliasId,
	yes_i_want_to_do_this: bool,
) -> Result {
	if !yes_i_want_to_do_this {
		return Err!(
			"You must pass the --yes-i-want-to-do-this-flag to ensure you really want to force \
			 bulk join all local users.",
		);
	}

	let server_admins = self.services.admin.get_admins().await;

	if server_admins.is_empty() {
		return Err!("There are no admins set for this server.");
	}

	let (room_id, servers) = self
		.services
		.rooms
		.alias
		.resolve_with_servers(&room_id, None)
		.await?;

	if !self
		.services
		.rooms
		.state_cache
		.server_in_room(self.services.globals.server_name(), &room_id)
		.await
	{
		return Err!("We are not joined in this room.");
	}

	if !self
		.services
		.rooms
		.state_cache
		.room_members(&room_id)
		.ready_any(|user_id| server_admins.contains(&user_id.to_owned()))
		.await
	{
		return Err!("There is not a single server admin in the room.",);
	}

	let mut failed_joins: usize = 0;
	let mut successful_joins: usize = 0;

	for user_id in &self
		.services
		.users
		.list_local_users()
		.map(UserId::to_owned)
		.collect::<Vec<_>>()
		.await
	{
		match join_room_by_id_helper(
			self.services,
			user_id,
			&room_id,
			Some(String::from(BULK_JOIN_REASON)),
			&servers,
			None,
			&None,
		)
		.await
		{
			| Ok(_res) => {
				successful_joins = successful_joins.saturating_add(1);
			},
			| Err(e) => {
				debug_warn!("Failed force joining {user_id} to {room_id} during bulk join: {e}");
				failed_joins = failed_joins.saturating_add(1);
			},
		}
	}

	self.write_str(&format!(
		"{successful_joins} local users have been joined to {room_id}. {failed_joins} joins \
		 failed.",
	))
	.await
}

#[admin_command]
pub(super) async fn force_join_room(
	&self,
	user_id: String,
	room_id: OwnedRoomOrAliasId,
) -> Result {
	let user_id = parse_local_user_id(self.services, &user_id)?;
	let (room_id, servers) = self
		.services
		.rooms
		.alias
		.resolve_with_servers(&room_id, None)
		.await?;

	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);
	join_room_by_id_helper(self.services, &user_id, &room_id, None, &servers, None, &None)
		.await?;

	self.write_str(&format!("{user_id} has been joined to {room_id}.",))
		.await
}

#[admin_command]
pub(super) async fn force_leave_room(
	&self,
	user_id: String,
	room_id: OwnedRoomOrAliasId,
) -> Result {
	let user_id = parse_local_user_id(self.services, &user_id)?;
	let room_id = self.services.rooms.alias.resolve(&room_id).await?;

	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);

	if !self
		.services
		.rooms
		.state_cache
		.is_joined(&user_id, &room_id)
		.await
	{
		return Err!("{user_id} is not joined in the room");
	}

	leave_room(self.services, &user_id, &room_id, None)
		.boxed()
		.await?;

	self.write_str(&format!("{user_id} has left {room_id}.",))
		.await
}

#[admin_command]
pub(super) async fn force_demote(&self, user_id: String, room_id: OwnedRoomOrAliasId) -> Result {
	let user_id = parse_local_user_id(self.services, &user_id)?;
	let room_id = self.services.rooms.alias.resolve(&room_id).await?;

	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);

	let state_lock = self.services.rooms.state.mutex.lock(&room_id).await;

	let room_power_levels: Option<RoomPowerLevelsEventContent> = self
		.services
		.rooms
		.state_accessor
		.room_state_get_content(&room_id, &StateEventType::RoomPowerLevels, "")
		.await
		.ok();

	let user_can_demote_self = room_power_levels
		.as_ref()
		.is_some_and(|power_levels_content| {
			RoomPowerLevels::from(power_levels_content.clone())
				.user_can_change_user_power_level(&user_id, &user_id)
		}) || self
		.services
		.rooms
		.state_accessor
		.room_state_get(&room_id, &StateEventType::RoomCreate, "")
		.await
		.is_ok_and(|event| event.sender() == user_id);

	if !user_can_demote_self {
		return Err!("User is not allowed to modify their own power levels in the room.",);
	}

	let mut power_levels_content = room_power_levels.unwrap_or_default();
	power_levels_content.users.remove(&user_id);

	let event_id = self
		.services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &power_levels_content),
			&user_id,
			Some(&room_id),
			&state_lock,
		)
		.await?;

	self.write_str(&format!(
		"User {user_id} demoted themselves to the room default power level in {room_id} - \
		 {event_id}"
	))
	.await
}

#[admin_command]
pub(super) async fn make_user_admin(&self, user_id: String) -> Result {
	let user_id = parse_local_user_id(self.services, &user_id)?;
	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);

	self.services
		.admin
		.make_user_admin(&user_id)
		.boxed()
		.await?;

	self.write_str(&format!("{user_id} has been granted admin privileges.",))
		.await
}

#[admin_command]
pub(super) async fn put_room_tag(
	&self,
	user_id: String,
	room_id: OwnedRoomId,
	tag: String,
) -> Result {
	let user_id = parse_active_local_user_id(self.services, &user_id).await?;

	let mut tags_event = self
		.services
		.account_data
		.get_room(&room_id, &user_id, RoomAccountDataEventType::Tag)
		.await
		.unwrap_or(TagEvent {
			content: TagEventContent { tags: BTreeMap::new() },
		});

	tags_event
		.content
		.tags
		.insert(tag.clone().into(), TagInfo::new());

	self.services
		.account_data
		.update(
			Some(&room_id),
			&user_id,
			RoomAccountDataEventType::Tag,
			&serde_json::to_value(tags_event).expect("to json value always works"),
		)
		.await?;

	self.write_str(&format!(
		"Successfully updated room account data for {user_id} and room {room_id} with tag {tag}"
	))
	.await
}

#[admin_command]
pub(super) async fn delete_room_tag(
	&self,
	user_id: String,
	room_id: OwnedRoomId,
	tag: String,
) -> Result {
	let user_id = parse_active_local_user_id(self.services, &user_id).await?;

	let mut tags_event = self
		.services
		.account_data
		.get_room(&room_id, &user_id, RoomAccountDataEventType::Tag)
		.await
		.unwrap_or(TagEvent {
			content: TagEventContent { tags: BTreeMap::new() },
		});

	tags_event.content.tags.remove(&tag.clone().into());

	self.services
		.account_data
		.update(
			Some(&room_id),
			&user_id,
			RoomAccountDataEventType::Tag,
			&serde_json::to_value(tags_event).expect("to json value always works"),
		)
		.await?;

	self.write_str(&format!(
		"Successfully updated room account data for {user_id} and room {room_id}, deleting room \
		 tag {tag}"
	))
	.await
}

#[admin_command]
pub(super) async fn get_room_tags(&self, user_id: String, room_id: OwnedRoomId) -> Result {
	let user_id = parse_active_local_user_id(self.services, &user_id).await?;

	let tags_event = self
		.services
		.account_data
		.get_room(&room_id, &user_id, RoomAccountDataEventType::Tag)
		.await
		.unwrap_or(TagEvent {
			content: TagEventContent { tags: BTreeMap::new() },
		});

	self.write_str(&format!("```\n{:#?}\n```", tags_event.content.tags))
		.await
}

#[admin_command]
pub(super) async fn redact_event(&self, event_id: OwnedEventId) -> Result {
	let Ok(event) = self
		.services
		.rooms
		.timeline
		.get_non_outlier_pdu(&event_id)
		.await
	else {
		return Err!("Event does not exist in our database.");
	};

	if event.is_redacted() {
		return Err!("Event is already redacted.");
	}

	if !self.services.globals.user_is_local(event.sender()) {
		return Err!("This command only works on local users.");
	}

	let reason = format!(
		"The administrator(s) of {} has redacted this user's message.",
		self.services.globals.server_name()
	);

	let redaction_event_id = {
		let state_lock = self
			.services
			.rooms
			.state
			.mutex
			.lock(&event.room_id_or_hash())
			.await;

		self.services
			.rooms
			.timeline
			.build_and_append_pdu(
				PduBuilder {
					redacts: Some(event.event_id().to_owned()),
					..PduBuilder::timeline(&RoomRedactionEventContent {
						redacts: Some(event.event_id().to_owned()),
						reason: Some(reason),
					})
				},
				event.sender(),
				Some(&event.room_id_or_hash()),
				&state_lock,
			)
			.await?
	};

	self.write_str(&format!(
		"Successfully redacted event. Redaction event ID: {redaction_event_id}"
	))
	.await
}

#[admin_command]
pub(super) async fn force_leave_remote_room(
	&self,
	user_id: String,
	room_id: OwnedRoomOrAliasId,
	via: Option<String>,
) -> Result {
	let user_id = parse_local_user_id(self.services, &user_id)?;
	let (room_id, vias_raw) = self
		.services
		.rooms
		.alias
		.resolve_with_servers(
			&room_id,
			if let Some(v) = via.clone() {
				Some(vec![OwnedServerName::parse(v)?])
			} else {
				None
			},
		)
		.await?;

	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);
	let mut vias: HashSet<OwnedServerName> = HashSet::new();
	if let Some(via) = via {
		vias.insert(OwnedServerName::parse(via)?);
	}
	for server in vias_raw {
		vias.insert(server);
	}
	remote_leave_room(self.services, &user_id, &room_id, None, vias)
		.boxed()
		.await?;

	self.write_str(&format!("{user_id} successfully left {room_id} via remote server."))
		.await
}

#[admin_command]
pub(super) async fn lock(&self, user_id: String) -> Result {
	self.bail_restricted()?;
	let user_id = parse_local_user_id(self.services, &user_id)?;
	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);
	if user_id == self.services.globals.server_user {
		return Err!("Not allowed to lock the server service account.",);
	}

	if !self.services.users.exists(&user_id).await {
		return Err!("User {user_id} does not exist.");
	}
	if self.services.users.is_admin(&user_id).await {
		return Err!("Admin users cannot be locked.");
	}
	self.services
		.users
		.lock_account(&user_id, self.sender_or_service_user())
		.await;

	self.write_str(&format!("User {user_id} has been locked."))
		.await
}

#[admin_command]
pub(super) async fn unlock(&self, user_id: String) -> Result {
	self.bail_restricted()?;
	let user_id = parse_local_user_id(self.services, &user_id)?;
	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);
	self.services.users.unlock_account(&user_id).await;

	self.write_str(&format!("User {user_id} has been unlocked."))
		.await
}

#[admin_command]
pub(super) async fn logout(&self, user_id: String) -> Result {
	self.bail_restricted()?;
	let user_id = parse_local_user_id(self.services, &user_id)?;
	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);
	if user_id == self.services.globals.server_user {
		return Err!("Not allowed to log out the server service account.",);
	}

	if !self.services.users.exists(&user_id).await {
		return Err!("User {user_id} does not exist.");
	}
	if self.services.users.is_admin(&user_id).await {
		return Err!("You cannot forcefully log out admin users.");
	}
	self.services
		.users
		.all_device_ids(&user_id)
		.for_each(|device_id| self.services.users.remove_device(&user_id, device_id))
		.await;
	self.write_str(&format!("User {user_id} has been logged out from all devices."))
		.await
}

#[admin_command]
pub(super) async fn disable_login(&self, user_id: String) -> Result {
	self.bail_restricted()?;
	let user_id = parse_local_user_id(self.services, &user_id)?;
	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);
	if user_id == self.services.globals.server_user {
		return Err!("Not allowed to disable login for the server service account.",);
	}

	if !self.services.users.exists(&user_id).await {
		return Err!("User {user_id} does not exist.");
	}
	if self.services.users.is_admin(&user_id).await {
		return Err!("Admin users cannot have their login disallowed.");
	}
	self.services.users.disable_login(&user_id);

	self.write_str(&format!(
		"{user_id} can no longer log in. Their existing sessions remain unaffected."
	))
	.await
}

#[admin_command]
pub(super) async fn enable_login(&self, user_id: String) -> Result {
	self.bail_restricted()?;
	let user_id = parse_local_user_id(self.services, &user_id)?;
	assert!(
		self.services.globals.user_is_local(&user_id),
		"Parsed user_id must be a local user"
	);
	if !self.services.users.exists(&user_id).await {
		return Err!("User {user_id} does not exist.");
	}
	self.services.users.enable_login(&user_id);

	self.write_str(&format!("{user_id} can now log in.")).await
}
