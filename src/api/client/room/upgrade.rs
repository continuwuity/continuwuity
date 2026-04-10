use std::cmp::max;

use axum::extract::State;
use conduwuit::{
	Err, Error, Event, Result, debug, err, info,
	matrix::{StateKey, pdu::PartialPdu},
};
use futures::{FutureExt, StreamExt};
use ruma::{
	CanonicalJsonObject, RoomId, RoomVersionId,
	api::{client::room::upgrade_room, error::ErrorKind},
	assign,
	events::{
		StateEventType, TimelineEventType,
		room::{
			create::PreviousRoom,
			member::{MembershipState, RoomMemberEventContent},
			power_levels::RoomPowerLevelsEventContent,
			tombstone::RoomTombstoneEventContent,
		},
		space::child::{RedactedSpaceChildEventContent, SpaceChildEventContent},
	},
	int,
	room_version_rules::RoomIdFormatVersion,
};
use serde_json::{json, value::to_raw_value};

use crate::router::Ruma;

/// Recommended transferable state events list from the spec
const TRANSFERABLE_STATE_EVENTS: &[StateEventType; 11] = &[
	StateEventType::RoomAvatar,
	StateEventType::RoomEncryption,
	StateEventType::RoomGuestAccess,
	StateEventType::RoomHistoryVisibility,
	StateEventType::RoomJoinRules,
	StateEventType::RoomName,
	StateEventType::RoomPowerLevels,
	StateEventType::RoomServerAcl,
	StateEventType::RoomTopic,
	// Not explicitly recommended in spec, but very useful.
	StateEventType::SpaceChild,
	StateEventType::SpaceParent, // TODO: m.room.policy?
];

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
	// TODO[v12]: Handle additional creators
	let sender_user = body.sender_user.as_ref().expect("user is authenticated");

	if !services.server.supported_room_version(&body.new_version) {
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

	// First, check if the user has permission to upgrade the room (send tombstone
	// event)
	let old_room_state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	// Check tombstone permission by attempting to create (but not send) the event
	// Note that this does internally call the policy server with a fake room ID,
	// which may not be good?
	let tombstone_test_result = services
		.rooms
		.timeline
		.create_hash_and_sign_event(
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
		.await;

	if let Err(_e) = tombstone_test_result {
		return Err!(Request(Forbidden("User does not have permission to upgrade this room.")));
	}

	drop(old_room_state_lock);

	// Create a replacement room
	let room_version_rules = body
		.new_version
		.rules()
		.expect("new room version should have defined rules");
	let replacement_room_owned = if room_version_rules.room_id_format == RoomIdFormatVersion::V2 {
		Some(RoomId::new_v1(services.globals.server_name()))
	} else {
		None
	};
	let replacement_room: Option<&RoomId> = replacement_room_owned.as_ref().map(AsRef::as_ref);
	let replacement_room_tmp = match replacement_room {
		| Some(v) => v,
		| None => &RoomId::new_v1(services.globals.server_name()),
	};

	let _short_id = services
		.rooms
		.short
		.get_or_create_shortroomid(replacement_room_tmp)
		.await;

	// For pre-v12 rooms, send tombstone before creating replacement room
	let tombstone_event_id = if room_version_rules.room_id_format != RoomIdFormatVersion::V2 {
		let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;
		// Send a m.room.tombstone event to the old room to indicate that it is not
		// intended to be used any further
		let tombstone_event_id = services
			.rooms
			.timeline
			.build_and_append_pdu(
				PartialPdu::state(
					StateKey::new(),
					&RoomTombstoneEventContent::new(
						"This room has been replaced".to_owned(),
						replacement_room.unwrap().to_owned(),
					),
				),
				sender_user,
				Some(&body.room_id),
				&state_lock,
			)
			.boxed()
			.await?;
		// Change lock to replacement room
		drop(state_lock);
		Some(tombstone_event_id)
	} else {
		None
	};
	let state_lock = services
		.rooms
		.state
		.mutex
		.lock(replacement_room_tmp.as_str())
		.await;

	// Get the old room creation event
	let mut create_event_content: CanonicalJsonObject = services
		.rooms
		.state_accessor
		.room_state_get_content(&body.room_id, &StateEventType::RoomCreate, "")
		.await
		.map_err(|_| err!(Database("Found room without m.room.create event.")))?;

	// Use the m.room.tombstone event as the predecessor

	let predecessor = {
		#[allow(deprecated, reason = "Clients still use event_id even though it's deprecated")]
		Some(assign!(PreviousRoom::new(body.room_id.clone()), {
			event_id: tombstone_event_id,
		}))
	};

	// Send a m.room.create event containing a predecessor field and the applicable
	// room_version
	{
		use RoomVersionId::*;
		match body.new_version {
			| V1 | V2 | V3 | V4 | V5 | V6 | V7 | V8 | V9 | V10 => {
				create_event_content.insert(
					"creator".into(),
					json!(&sender_user).try_into().map_err(|e| {
						info!("Error forming creation event: {e}");
						Error::BadRequest(ErrorKind::BadJson, "Error forming creation event")
					})?,
				);
			},
			| _ => {
				// "creator" key no longer exists in V11 rooms
				create_event_content.remove("creator");
			},
			// TODO(hydra): additional_creators
		}
	}

	create_event_content.insert(
		"room_version".into(),
		json!(&body.new_version)
			.try_into()
			.map_err(|_| Error::BadRequest(ErrorKind::BadJson, "Error forming creation event"))?,
	);
	create_event_content.insert(
		"predecessor".into(),
		json!(predecessor)
			.try_into()
			.map_err(|_| Error::BadRequest(ErrorKind::BadJson, "Error forming creation event"))?,
	);

	// Validate creation event content
	if serde_json::from_str::<CanonicalJsonObject>(
		to_raw_value(&create_event_content)
			.expect("Error forming creation event")
			.get(),
	)
	.is_err()
	{
		return Err(Error::BadRequest(ErrorKind::BadJson, "Error forming creation event"));
	}

	let create_event_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu {
				event_type: TimelineEventType::RoomCreate,
				content: to_raw_value(&create_event_content)
					.expect("event is valid, we just created it"),
				unsigned: None,
				state_key: Some(StateKey::new()),
				redacts: None,
				timestamp: None,
			},
			sender_user,
			replacement_room,
			&state_lock,
		)
		.boxed()
		.await?;
	let create_id = create_event_id.as_str().replace('$', "!");
	let (replacement_room, state_lock) =
		if room_version_rules.room_id_format == RoomIdFormatVersion::V2 {
			let parsed_room_id = RoomId::parse(&create_id)?;
			let lock = services
				.rooms
				.state
				.mutex
				.lock(parsed_room_id.as_str())
				.await;
			(Some(parsed_room_id), lock)
		} else {
			(replacement_room.map(ToOwned::to_owned), state_lock)
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
					blurhash: services.users.blurhash(sender_user).await.ok(),
				}),
			),
			sender_user,
			replacement_room.as_deref(),
			&state_lock,
		)
		.boxed()
		.await?;

	// Replicate transferable state events to the new room
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
			if event_content.get() == "{}" {
				// If the event content is empty, we skip it
				continue;
			}
			// If this is a power levels event, and the new room version has creators,
			// we need to make sure they dont appear in the users block of power levels.
			if *event_type == StateEventType::RoomPowerLevels {
				// TODO(v12): additional creators
				let creators = vec![sender_user];
				let mut power_levels_event_content: RoomPowerLevelsEventContent =
					serde_json::from_str(event_content.get()).map_err(|_| {
						err!(Request(BadJson("Power levels event content is not valid")))
					})?;
				for creator in creators {
					power_levels_event_content.users.remove(creator);
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
					replacement_room.as_deref(),
					&state_lock,
				)
				.boxed()
				.await?;
		}
	}

	// Moves any local aliases to the new room
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
			replacement_room.as_ref().unwrap(),
			sender_user,
		)?;
	}

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

	// Modify the power levels in the old room to prevent sending of events and
	// inviting new users
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
			&state_lock,
		)
		.boxed()
		.await?;

	drop(state_lock);

	// For v12 rooms, send tombstone AFTER creating replacement room
	if room_version_rules.room_id_format == RoomIdFormatVersion::V2 {
		let old_room_state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;
		// For v12 rooms, no event reference in predecessor due to cyclic dependency -
		// could best effort one maybe?
		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PartialPdu::state(
					StateKey::new(),
					&RoomTombstoneEventContent::new(
						"This room has been replaced".to_owned(),
						replacement_room.as_ref().unwrap().to_owned(),
					),
				),
				sender_user,
				Some(&body.room_id),
				&old_room_state_lock,
			)
			.await?;
		drop(old_room_state_lock);
	}

	// Check if the old room has a space parent, and if so, whether we should update
	// it (m.space.parent, room_id)
	let parents = services
		.rooms
		.state_accessor
		.room_state_keys(&body.room_id, &StateEventType::SpaceParent)
		.await?;

	for raw_space_id in parents {
		let space_id = RoomId::parse(&raw_space_id)?;
		let Ok(child) = services
			.rooms
			.state_accessor
			.room_state_get_content::<SpaceChildEventContent>(
				&space_id,
				&StateEventType::SpaceChild,
				body.room_id.as_str(),
			)
			.await
		else {
			// If the space does not have a child event for this room, we can skip it
			continue;
		};
		debug!(
			"Updating space {space_id} child event for room {} to {}",
			&body.room_id,
			replacement_room.as_ref().unwrap()
		);
		// First, drop the space's child event
		let state_lock = services.rooms.state.mutex.lock(space_id.as_str()).await;
		debug!("Removing space child event for room {} in space {space_id}", &body.room_id);
		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PartialPdu {
					event_type: StateEventType::SpaceChild.into(),
					content: to_raw_value(&RedactedSpaceChildEventContent::new())
						.expect("event is valid, we just created it"),
					state_key: Some(body.room_id.clone().as_str().into()),
					..Default::default()
				},
				sender_user,
				Some(&space_id),
				&state_lock,
			)
			.boxed()
			.await
			.ok();
		// Now, add a new child event for the replacement room
		debug!(
			"Adding space child event for room {} in space {space_id}",
			replacement_room.as_ref().unwrap()
		);
		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PartialPdu::state(
					replacement_room.as_ref().unwrap().as_str(),
					&assign!(SpaceChildEventContent::new(vec![sender_user.server_name().to_owned()]), {
						order: child.order,
						suggested: child.suggested,
					}),
				),
				sender_user,
				Some(&space_id),
				&state_lock,
			)
			.boxed()
			.await
			.ok();
		debug!(
			"Finished updating space {space_id} child event for room {} to {}",
			&body.room_id,
			replacement_room.as_ref().unwrap()
		);
		drop(state_lock);
	}

	// Return the replacement room id
	Ok(upgrade_room::v3::Response::new(replacement_room.as_ref().unwrap().to_owned()))
}
