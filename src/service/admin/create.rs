use conduwuit::{Result, info, pdu::PartialPdu};
use futures::FutureExt;
use ruma::{
	Int, RoomId,
	events::{
		TimelineEventType,
		room::{
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
	},
	room_version_rules::RoomIdFormatVersion,
};

use crate::Services;

/// Create the admin room.
///
/// Users in this room are considered admins by conduwuit, and the room can be
/// used to issue admin commands by talking to the server user inside it.
pub async fn create_admin_room(services: &Services) -> Result {
	let room_version = services.config.default_room_version.clone();
	let room_version_rules = room_version
		.rules()
		.expect("default_room_version must be supported");
	let room_id = match room_version_rules.room_id_format {
		| RoomIdFormatVersion::V1 => {
			let room_id = RoomId::new_v1(services.globals.server_name());
			services
				.rooms
				.short
				.get_or_create_shortroomid(&room_id)
				.await;
			Some(room_id)
		},
		| RoomIdFormatVersion::V2 => None,
		| _ => panic!("Unknown room version format"),
	};

	let state_lock = services.rooms.state.mutex.lock("!new-room").await;

	// Create a user for the server
	let server_user = services.globals.server_user.as_ref();
	services.users.create(server_user, None).await?;

	let mut create_content = if room_version_rules.authorization.use_room_create_sender {
		RoomCreateEventContent::new_v1(server_user.into())
	} else {
		RoomCreateEventContent::new_v11()
	};

	create_content.federate = true;
	create_content.room_version = room_version.clone();

	info!(
		"Creating admin room {} with version {}",
		room_id
			.clone()
			.map_or_else(|| "<not known ahead of time>".to_owned(), |id| id.as_str().to_owned()),
		room_version
	);

	// 1. The room create event
	let create_event_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(String::new(), &create_content),
			server_user,
			room_id.as_deref(),
			&state_lock,
		)
		.boxed()
		.await?;
	let room_id = room_id.unwrap_or_else(|| {
		RoomId::new_v2(
			create_event_id
				.as_str()
				.strip_prefix("$")
				.expect("event ID must start with a $ sigil"),
		)
		.expect("event ID without sigil must be a valid room ID")
	});

	// 2. Make server user/bot join
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(
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
	let mut power_levels_content =
		RoomPowerLevelsEventContent::new(&room_version_rules.authorization);
	if !room_version_rules
		.authorization
		.explicitly_privilege_room_creators
	{
		power_levels_content
			.users
			.insert(server_user.into(), Int::MAX);
	}
	// Prevent common foot-shotguns
	power_levels_content
		.events
		.insert(TimelineEventType::RoomTombstone, Int::MAX);
	power_levels_content
		.events
		.insert(TimelineEventType::RoomEncryption, Int::MAX);
	power_levels_content
		.events
		.insert(TimelineEventType::RoomCanonicalAlias, Int::MAX);
	power_levels_content.invite = power_levels_content.state_default;

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(String::new(), &power_levels_content),
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
			PartialPdu::state(String::new(), &RoomJoinRulesEventContent::new(JoinRule::Invite)),
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
			PartialPdu::state(
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
			PartialPdu::state(
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
			PartialPdu::state(String::new(), &RoomNameEventContent::new(room_name)),
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
			PartialPdu::state(String::new(), &RoomTopicEventContent::markdown(room_topic)),
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
			PartialPdu::state(String::new(), &alias_content),
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
