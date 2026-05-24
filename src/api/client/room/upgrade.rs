use std::cmp::max;

use axum::extract::State;
use conduwuit::{
	Err, Error, Event, Result, err,
	info::room_version::UNSTABLE_ROOM_VERSIONS,
	matrix::{StateKey, pdu::PartialPdu},
};
use futures::{FutureExt, StreamExt};
use ruma::{
	OwnedEventId, OwnedRoomId, RoomId, UserId,
	api::{client::room::upgrade_room, error::ErrorKind},
	assign,
	events::{
		StateEventType,
		room::{
			create::{PreviousRoom, RoomCreateEventContent},
			member::{MembershipState, RoomMemberEventContent},
			power_levels::RoomPowerLevelsEventContent,
			tombstone::RoomTombstoneEventContent,
		},
		space::{child::SpaceChildEventContent, parent::SpaceParentEventContent},
	},
	int,
	room_version_rules::RoomIdFormatVersion,
};
use serde_json::value::to_raw_value;

use crate::router::Ruma;

/// Recommended transferable state events list from the spec
const TRANSFERABLE_STATE_EVENTS: &[StateEventType; 11] = &[
	StateEventType::RoomServerAcl,
	StateEventType::RoomEncryption,
	StateEventType::RoomName,
	StateEventType::RoomAvatar,
	StateEventType::RoomTopic,
	StateEventType::RoomGuestAccess,
	StateEventType::RoomHistoryVisibility,
	StateEventType::RoomJoinRules,
	StateEventType::RoomPowerLevels,
	// MSC4168: https://github.com/matrix-org/matrix-spec-proposals/pull/4168
	StateEventType::SpaceChild,
	StateEventType::SpaceParent,
];

/// Updates spaces that are marked as parents of old_room_id to instead point to
/// the new room ID.
///
/// See: https://github.com/matrix-org/matrix-spec-proposals/pull/4168
async fn msc4168_update_parent_spaces(
	services: &crate::State,
	sender: &UserId,
	old_room_id: &RoomId,
	new_room_id: &RoomId,
) -> Result {
	// Fetch the spaces which this room claims are its parents.

	// In rooms that reference the old room via m.space.child events...
	let parents = services
		.rooms
		.state_accessor
		.room_state_keys(old_room_id, &StateEventType::SpaceParent)
		.await?;

	for raw_parent_id in parents {
		let parent_id = RoomId::parse(&raw_parent_id)?;
		let state_lock = services.rooms.state.mutex.lock(parent_id.as_str()).await;
		// We're now fetching state from the *space* that has the old room as a *child*.
		// Follow along. This will be on the test.
		let Ok(child) = services
			.rooms
			.state_accessor
			.room_state_get_content::<SpaceChildEventContent>(
				&parent_id,
				&StateEventType::SpaceChild,
				old_room_id.as_str(),
			)
			.await
		else {
			// If the space does not have a child event for this room, we can skip it
			continue;
		};

		// ...the upgrading server SHOULD send a new m.space.child event with state_key
		// set to the new room's ID, copying the order and suggested fields from the
		// content of the m.space.child with state_key of the previous room ID.
		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PartialPdu::state(
					new_room_id.as_str(),
					&assign!(
						SpaceChildEventContent::new(vec![sender.server_name().to_owned()]),
						{
							order: child.order,
							suggested: child.suggested,
						}
					),
				),
				sender,
				Some(&parent_id),
				&state_lock,
			)
			.boxed()
			.await
			.ok();
		drop(state_lock);
	}

	Ok(())
}

/// If the room being upgraded is a space, replace all m.space.parent references
/// in its children to point at the newly upgraded room ID, so that they point
/// at the new space.
///
/// See: https://github.com/matrix-org/matrix-spec-proposals/pull/4168
async fn msc4168_update_space_children(
	services: &crate::State,
	sender: &UserId,
	old_room_id: &RoomId,
	new_room_id: &RoomId,
) -> Result {
	// Fetch the children of this space.
	// Note that this might not actually be a space, but just a room that has
	// children.

	// In rooms that reference the old room via m.space.parent events...
	let parents = services
		.rooms
		.state_accessor
		.room_state_keys(old_room_id, &StateEventType::SpaceParent)
		.await?;

	for raw_child_id in parents {
		let child_id = RoomId::parse(&raw_child_id)?;
		let state_lock = services.rooms.state.mutex.lock(child_id.as_str()).await;
		// We're now fetching state from the *child* that has the old space as a
		// *parent*. Follow along. This will also be on the test.
		let Ok(ref parent) = services
			.rooms
			.state_accessor
			.room_state_get_content::<SpaceParentEventContent>(
				&child_id,
				&StateEventType::SpaceParent,
				old_room_id.as_str(),
			)
			.await
		else {
			// If the child does not have a parent event for this room, we can skip it.
			continue;
		};

		// ... the upgrading server SHOULD send a new m.space.parent event with
		// state_key set to the new room's ID.
		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PartialPdu::state(
					new_room_id.as_str(),
					&assign!(SpaceParentEventContent::new(vec![sender.server_name().to_owned()]), { canonical: parent.canonical }),
				),
				sender,
				Some(&child_id),
				&state_lock,
			)
			.boxed()
			.await
			.ok();

		// If the previous m.space.parent event has canonical set to true in content,
		// homeservers SHOULD update the old state event to set canonical to false,
		// while setting it to true in the newly-sent m.space.parent event.
		if parent.canonical {
			services
				.rooms
				.timeline
				.build_and_append_pdu(
					PartialPdu {
						event_type: StateEventType::SpaceParent.into(),
						content: to_raw_value(&assign!(parent.clone(), {canonical: false}))
							.expect("event is valid, we just created it"),
						state_key: Some(old_room_id.as_str().into()),
						..Default::default()
					},
					sender,
					Some(&child_id),
					&state_lock,
				)
				.boxed()
				.await
				.ok();
		}
		drop(state_lock);
	}

	Ok(())
}

/// # `POST /_matrix/client/r0/rooms/{roomId}/upgrade`
///
/// Upgrades the room.
///
/// - Creates a replacement room
/// - Sends a tombstone event into the current room
/// - Sender user joins the room
/// - Transfers some state events
/// - Moves local aliases
/// - Modifies old room power levels to prevent users from speaking
pub(crate) async fn upgrade_room_route(
	State(services): State<crate::State>,
	body: Ruma<upgrade_room::v3::Request>,
) -> Result<upgrade_room::v3::Response> {
	let sender_user = body.sender_user();

	if !services.server.supported_room_version(&body.new_version) {
		return Err(Error::BadRequest(
			ErrorKind::UnsupportedRoomVersion,
			"This server does not support that room version.",
		));
	}
	if !services.config.allow_unstable_room_versions
		&& UNSTABLE_ROOM_VERSIONS.contains(&body.new_version)
	{
		return Err(Error::BadRequest(
			ErrorKind::UnsupportedRoomVersion,
			"This server does not support that room version.",
		));
	}

	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	// Make sure this isn't the admin room
	// Admin room upgrades are hacky and should be done manually instead.
	if services.admin.is_admin_room(&body.room_id).await {
		return Err!(Request(Forbidden("Upgrading the admin room this way is not allowed.")));
	}

	// 1. Check that the user has permission to send m.room.tombstone events in the
	//    room.
	let old_room_state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	// Check tombstone permission by attempting to create (but not send) the event.
	services
		.rooms
		.timeline
		.create_event(
			PartialPdu::state(
				StateKey::new(),
				&RoomTombstoneEventContent::new(
					String::new(),
					RoomId::new_v1(services.globals.server_name()),
				),
			),
			sender_user,
			Some(&body.room_id),
			&old_room_state_lock,
		)
		.await
		.map_err(|_| {
			err!(Request(Forbidden("You do not have permission to upgrade this room.")))
		})?;

	// Create a replacement room
	let new_version_rules = body
		.new_version
		.rules()
		.expect("new room version should have defined rules");

	let last_event = if new_version_rules
		.authorization
		.room_create_event_id_as_room_id
	{
		None
	} else {
		Some(
			services
				.rooms
				.state
				.get_forward_extremities(&body.room_id)
				.collect::<Vec<OwnedEventId>>()
				.await[0]
				.clone(),
		)
	};
	let old_create_event: RoomCreateEventContent = services
		.rooms
		.state_accessor
		.room_state_get_content(&body.room_id, &StateEventType::RoomCreate, "")
		.await
		.map_err(|_| err!(Database("Found room without m.room.create event.")))?;
	let create_event_content = if new_version_rules.authorization.use_room_create_sender {
		RoomCreateEventContent::new_v1(sender_user.to_owned())
	} else {
		RoomCreateEventContent::new_v11()
	};
	#[allow(deprecated)]
	let create_event_content = {
		assign!(
			create_event_content,
			{
				additional_creators: if new_version_rules.authorization.additional_room_creators {
					body.additional_creators.clone()
				} else { Vec::new() },
				creator: if new_version_rules.authorization.use_room_create_sender {
					None
				} else { Some(sender_user.to_owned()) },
				predecessor: Some(assign!(PreviousRoom::new(body.room_id.clone()), {
					event_id: last_event,
				})),
				room_type: old_create_event.room_type.clone(),
				room_version: body.new_version.clone(),
			}
		)
	};

	let replacement_room_id: Option<OwnedRoomId> =
		if new_version_rules.room_id_format == RoomIdFormatVersion::V2 {
			None
		} else {
			Some(RoomId::new_v1(services.globals.server_name()))
		};

	let new_room_state_lock = if let Some(new_room_id) = replacement_room_id.as_ref() {
		services.rooms.state.mutex.lock(new_room_id.as_str()).await
	} else {
		// NOTE: Using a hardcoded room ID for the temporary mutex means only one room
		// can be created at a time. This is actually beneficial, as it reduces the
		// risk of concurrent in-flight collisions.
		services.rooms.state.mutex.lock("!new-room").await
	};
	let create_event_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(StateKey::new(), &create_event_content),
			sender_user,
			replacement_room_id.as_deref(),
			&new_room_state_lock,
		)
		.boxed()
		.await?;
	drop(new_room_state_lock);
	// re-acquire a new lock with the new room ID.
	// We don't actually need a state lock for sending the m.room.create event, but
	// we get one anyway because the function requires it and I can't be bothered
	// refactoring it.
	let (replacement_room_id, new_room_state_lock) =
		if new_version_rules.room_id_format == RoomIdFormatVersion::V2 {
			let parsed_room_id = RoomId::new_v2(
				create_event_id
					.as_str()
					.strip_prefix("$")
					.expect("event ID must start with $ sigil"),
			)?;
			let lock = services
				.rooms
				.state
				.mutex
				.lock(parsed_room_id.as_str())
				.await;
			(Some(parsed_room_id), lock)
		} else {
			let new_room_id =
				replacement_room_id.expect("replacement room id should be known by now");
			let lock = services.rooms.state.mutex.lock(new_room_id.as_str()).await;
			(Some(new_room_id), lock)
		};

	// Join the new room
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(
				sender_user.as_str(),
				&assign!(RoomMemberEventContent::new(MembershipState::Join), {
					displayname: services.users.displayname(sender_user).await.ok(),
					avatar_url: services.users.avatar_url(sender_user).await.ok(),
				}),
			),
			sender_user,
			replacement_room_id.as_deref(),
			&new_room_state_lock,
		)
		.boxed()
		.await?;

	// 3. Replicate transferable state events to the new room
	for event_type in TRANSFERABLE_STATE_EVENTS {
		let state_keys = services
			.rooms
			.state_accessor
			.room_state_keys(&body.room_id, event_type)
			.await?;
		for state_key in state_keys {
			let mut event_content = match services
				.rooms
				.state_accessor
				.room_state_get(&body.room_id, event_type, &state_key)
				.await
			{
				| Ok(v) => v.content().to_owned(),
				| Err(_) => continue, // Skipping missing events.
			};
			// If this is a power levels event, and the new room version has creators,
			// we need to make sure they dont appear in the users block of power levels.
			if *event_type == StateEventType::RoomPowerLevels {
				let creators = body
					.additional_creators
					.clone()
					.iter()
					.chain(std::iter::once(&sender_user.to_owned()))
					.map(ToOwned::to_owned)
					.collect::<Vec<_>>();
				let mut power_levels_event_content: RoomPowerLevelsEventContent =
					serde_json::from_str(event_content.get()).map_err(|_| {
						err!(Request(BadJson("Power levels event content is not valid")))
					})?;
				for creator in creators {
					if new_version_rules
						.authorization
						.explicitly_privilege_room_creators
					{
						power_levels_event_content.users.remove(&creator);
					} else {
						power_levels_event_content.users.insert(
							creator.clone(),
							max(
								int!(100),
								power_levels_event_content
									.users
									.get(&creator)
									.copied()
									.unwrap_or_default(),
							),
						);
					}
				}
				event_content = to_raw_value(&power_levels_event_content)
					.expect("event is valid, we just deserialized and modified it");
			}

			services
				.rooms
				.timeline
				.build_and_append_pdu(
					PartialPdu {
						event_type: event_type.to_string().into(),
						content: event_content,
						state_key: Some(StateKey::from(state_key)),
						..Default::default()
					},
					sender_user,
					replacement_room_id.as_deref(),
					&new_room_state_lock,
				)
				.boxed()
				.await?;
		}
	}

	// 4. Move any local aliases to the new room
	let mut local_aliases = services
		.rooms
		.alias
		.local_aliases_for_room(&body.room_id)
		.boxed();

	while let Some(alias) = local_aliases.next().await {
		services
			.rooms
			.alias
			.remove_alias(&alias, sender_user)
			.await?;

		services.rooms.alias.set_alias(
			&alias,
			replacement_room_id.as_deref().unwrap(),
			sender_user,
		)?;
	}

	// 5. Send a `m.room.tombstone` event to the old room to indicate that it is not
	//    intended to be used any further.
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(
				StateKey::new(),
				&RoomTombstoneEventContent::new(
					"This room has been replaced".to_owned(),
					replacement_room_id.clone().unwrap(),
				),
			),
			sender_user,
			Some(&body.room_id),
			&old_room_state_lock,
		)
		.await?;

	// Get the old room power levels
	let mut power_levels = services
		.rooms
		.state_accessor
		.get_room_power_levels(&body.room_id)
		.await;

	// Setting events_default and invite to the greater of 50 and users_default + 1
	let new_level = max(
		int!(50),
		power_levels
			.users_default
			.checked_add(int!(1))
			.ok_or_else(|| {
				err!(Request(BadJson("users_default power levels event content is not valid")))
			})?,
	);

	power_levels.events_default = new_level;
	power_levels.invite = new_level;

	// 6. Modify the power levels in the old room to prevent sending of events and
	// inviting new users
	// Spec dictates that this is allowed to fail.
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(
				StateKey::new(),
				&RoomPowerLevelsEventContent::try_from(power_levels).unwrap(),
			),
			sender_user,
			Some(&body.room_id),
			&old_room_state_lock,
		)
		.boxed()
		.await
		.ok();

	// MSC4168: Update spaces that reference this room to point at the new room.
	msc4168_update_parent_spaces(
		&services,
		sender_user,
		&body.room_id,
		replacement_room_id.as_deref().unwrap(),
	)
	.await
	.ok();
	// MSC4168: Update child rooms to point at the new space, where possible
	msc4168_update_space_children(
		&services,
		sender_user,
		&body.room_id,
		replacement_room_id.as_deref().unwrap(),
	)
	.await
	.ok();

	// Return the replacement room id
	Ok(upgrade_room::v3::Response::new(replacement_room_id.unwrap()))
}
