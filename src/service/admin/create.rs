use std::collections::BTreeMap;

use conduwuit::{Result, info, pdu::PduBuilder};
use futures::FutureExt;
use ruma::{
	RoomId, RoomVersionId,
	events::room::{
		canonical_alias::RoomCanonicalAliasEventContent,
		create::RoomCreateEventContent,
		guest_access::{GuestAccess, RoomGuestAccessEventContent},
		history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
		join_rules::{JoinRule, RoomJoinRulesEventContent},
		member::{MembershipState, RoomMemberEventContent},
		name::RoomNameEventContent,
		power_levels::RoomPowerLevelsEventContent,
		topic::RoomTopicEventContent,
	},
};

use crate::Services;

/// Create the admin room.
///
/// Users in this room are considered admins by conduwuit, and the room can be
/// used to issue admin commands by talking to the server user inside it.
pub async fn create_admin_room(services: &Services) -> Result {
	let room_id = RoomId::new_v1(services.globals.server_name());
	let room_version = &RoomVersionId::V11;

	let _short_id = services
		.rooms
		.short
		.get_or_create_shortroomid(&room_id)
		.await;

	let state_lock = services.rooms.state.mutex.lock(room_id.as_str()).await;

	// Create a user for the server
	let server_user = services.globals.server_user.as_ref();
	services.users.create(server_user, None, None).await?;

	let mut create_content = {
		use RoomVersionId::*;
		match room_version {
			| V1 | V2 | V3 | V4 | V5 | V6 | V7 | V8 | V9 | V10 =>
				RoomCreateEventContent::new_v1(server_user.into()),
			| _ => RoomCreateEventContent::new_v11(),
		}
	};

	create_content.federate = true;
	create_content.room_version = room_version.clone();

	info!("Creating admin room {} with version {}", room_id, room_version);

	// 1. The room create event
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &create_content),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	// 2. Make server user/bot join
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(
				String::from(server_user),
				&RoomMemberEventContent::new(MembershipState::Join),
			),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	// 3. Power levels
	let users = BTreeMap::from_iter([(server_user.into(), 69420.into())]);

	let mut power_levels_content = RoomPowerLevelsEventContent::new(&room_version.rules().unwrap().authorization);
	power_levels_content.users = users;

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &power_levels_content),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	// 4.1 Join Rules
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &RoomJoinRulesEventContent::new(JoinRule::Invite)),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	// 4.2 History Visibility
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(
				String::new(),
				&RoomHistoryVisibilityEventContent::new(HistoryVisibility::Shared),
			),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	// 4.3 Guest Access
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(
				String::new(),
				&RoomGuestAccessEventContent::new(GuestAccess::Forbidden),
			),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	// 5. Events implied by name and topic
	let room_name = format!("{} Admin Room", services.config.server_name);
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &RoomNameEventContent::new(room_name)),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	let room_topic = format!("Manage {} | Run commands prefixed with `!admin` | Run `!admin -h` for help | Documentation: https://continuwuity.org/", services.config.server_name);
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &RoomTopicEventContent::markdown(room_topic)),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	// 6. Room alias
	let mut alias_content = RoomCanonicalAliasEventContent::new();
	alias_content.alias = Some(services.globals.admin_alias.clone());

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &alias_content),
			server_user,
			Some(&room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	services
		.rooms
		.alias
		.set_alias(&services.globals.admin_alias, &room_id, server_user)?;

	Ok(())
}
