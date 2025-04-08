use std::fmt::Write;

use conduwuit::Result;
use futures::StreamExt;
use ruma::{
	OwnedRoomId, OwnedServerName, OwnedUserId, events::room::message::RoomMessageEventContent,
};

use crate::{admin_command, get_room_info};

#[admin_command]
pub(super) async fn disable_room(&self, room_id: OwnedRoomId) -> Result<RoomMessageEventContent> {
	self.services.rooms.metadata.disable_room(&room_id, true);
	Ok(RoomMessageEventContent::text_plain("Room disabled."))
}

#[admin_command]
pub(super) async fn enable_room(&self, room_id: OwnedRoomId) -> Result<RoomMessageEventContent> {
	self.services.rooms.metadata.disable_room(&room_id, false);
	Ok(RoomMessageEventContent::text_plain("Room enabled."))
}

#[admin_command]
pub(super) async fn incoming_federation(&self) -> Result<RoomMessageEventContent> {
	let map = self
		.services
		.rooms
		.event_handler
		.federation_handletime
		.read()
		.expect("locked");
	let mut msg = format!("Handling {} incoming pdus:\n", map.len());

	for (r, (e, i)) in map.iter() {
		let elapsed = i.elapsed();
		writeln!(msg, "{} {}: {}m{}s", r, e, elapsed.as_secs() / 60, elapsed.as_secs() % 60)?;
	}

	Ok(RoomMessageEventContent::text_plain(&msg))
}

#[admin_command]
pub(super) async fn fetch_support_well_known(
	&self,
	server_name: OwnedServerName,
) -> Result<RoomMessageEventContent> {
	let response = self
		.services
		.client
		.default
		.get(format!("https://{server_name}/.well-known/matrix/support"))
		.send()
		.await?;

	let text = response.text().await?;

	if text.is_empty() {
		return Ok(RoomMessageEventContent::text_plain("Response text/body is empty."));
	}

	if text.len() > 1500 {
		return Ok(RoomMessageEventContent::text_plain(
			"Response text/body is over 1500 characters, assuming no support well-known.",
		));
	}

	let json: serde_json::Value = match serde_json::from_str(&text) {
		| Ok(json) => json,
		| Err(_) => {
			return Ok(RoomMessageEventContent::text_plain(
				"Response text/body is not valid JSON.",
			));
		},
	};

	let pretty_json: String = match serde_json::to_string_pretty(&json) {
		| Ok(json) => json,
		| Err(_) => {
			return Ok(RoomMessageEventContent::text_plain(
				"Response text/body is not valid JSON.",
			));
		},
	};

	Ok(RoomMessageEventContent::notice_markdown(format!(
		"Got JSON response:\n\n```json\n{pretty_json}\n```"
	)))
}

#[admin_command]
pub(super) async fn remote_user_in_rooms(
	&self,
	user_id: OwnedUserId,
) -> Result<RoomMessageEventContent> {
	if user_id.server_name() == self.services.server.name {
		return Ok(RoomMessageEventContent::text_plain(
			"User belongs to our server, please use `list-joined-rooms` user admin command \
			 instead.",
		));
	}

	if !self.services.users.exists(&user_id).await {
		return Ok(RoomMessageEventContent::text_plain(
			"Remote user does not exist in our database.",
		));
	}

	let mut rooms: Vec<(OwnedRoomId, u64, String)> = self
		.services
		.rooms
		.state_cache
		.rooms_joined(&user_id)
		.then(|room_id| get_room_info(self.services, room_id))
		.collect()
		.await;

	if rooms.is_empty() {
		return Ok(RoomMessageEventContent::text_plain("User is not in any rooms."));
	}

	rooms.sort_by_key(|r| r.1);
	rooms.reverse();

	let output = format!(
		"Rooms {user_id} shares with us ({}):\n```\n{}\n```",
		rooms.len(),
		rooms
			.iter()
			.map(|(id, members, name)| format!("{id} | Members: {members} | Name: {name}"))
			.collect::<Vec<_>>()
			.join("\n")
	);

	Ok(RoomMessageEventContent::text_markdown(output))
}
