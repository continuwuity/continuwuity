use std::cmp::max;

use axum::extract::State;
use conduwuit::{
	Err, Error, Event, Result, RoomVersion, debug, err, info,
	matrix::{StateKey, pdu::PduBuilder},
};
use futures::{FutureExt, StreamExt};
use ruma::{
	CanonicalJsonObject, RoomId, RoomVersionId,
	api::client::{error::ErrorKind, room::upgrade_room},
	events::{
		StateEventType, TimelineEventType,
		room::{
			member::{MembershipState, RoomMemberEventContent},
			power_levels::RoomPowerLevelsEventContent,
			tombstone::RoomTombstoneEventContent,
		},
		space::child::{RedactedSpaceChildEventContent, SpaceChildEventContent},
	},
	int,
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

	// Create a replacement room
	let room_features = RoomVersion::new(&body.new_version)?;
	let replacement_room: Option<&RoomId> = if room_features.room_ids_as_hashes {
		None
	} else {
		Some(&RoomId::new(services.globals.server_name()))
	};
	let replacement_room_tmp = match replacement_room {
		| Some(v) => v,
		| None => &RoomId::new(services.globals.server_name()),
	};

	let _short_id = services
		.rooms
		.short
		.get_or_create_shortroomid(replacement_room_tmp)
		.await;

	let tombstone_event_id = if room_features.room_ids_as_hashes {
		None
	} else {
		let state_lock = services.rooms.state.mutex.lock(&body.room_id).await;
		// Send a m.room.tombstone event to the old room to indicate that it is not
		// intended to be used any further Fail if the sender does not have the required
		// permissions
		let tombstone_event_id = services
			.rooms
			.timeline
			.build_and_append_pdu(
				PduBuilder::state(StateKey::new(), &RoomTombstoneEventContent {
					body: "This room has been replaced".to_owned(),
					replacement_room: replacement_room.clone().unwrap().to_owned(),
				}),
				sender_user,
				Some(&body.room_id),
				&state_lock,
			)
			.await?;
		// Change lock to replacement room
		drop(state_lock);
		Some(tombstone_event_id)
	};
	let state_lock = services.rooms.state.mutex.lock(replacement_room_tmp).await;

	// Get the old room creation event
	let mut create_event_content: CanonicalJsonObject = services
		.rooms
		.state_accessor
		.room_state_get_content(&body.room_id, &StateEventType::RoomCreate, "")
		.await
		.map_err(|_| err!(Database("Found room without m.room.create event.")))?;

	// Use the m.room.tombstone event as the predecessor
	let predecessor = Some(ruma::events::room::create::PreviousRoom::new(
		body.room_id.clone(),
		tombstone_event_id,
	));

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
			PduBuilder {
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
	let (replacement_room, state_lock) = if room_features.room_ids_as_hashes {
		let parsed_room_id = RoomId::parse(&create_id)?;
		(Some(parsed_room_id), services.rooms.state.mutex.lock(parsed_room_id).await)
	} else {
		(replacement_room, state_lock)
	};

	// Join the new room
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder {
				event_type: TimelineEventType::RoomMember,
				content: to_raw_value(&RoomMemberEventContent {
					membership: MembershipState::Join,
					displayname: services.users.displayname(sender_user).await.ok(),
					avatar_url: services.users.avatar_url(sender_user).await.ok(),
					is_direct: None,
					third_party_invite: None,
					blurhash: services.users.blurhash(sender_user).await.ok(),
					reason: None,
					join_authorized_via_users_server: None,
					redact_events: None,
				})
				.expect("event is valid, we just created it"),
				unsigned: None,
				state_key: Some(sender_user.as_str().into()),
				redacts: None,
				timestamp: None,
			},
			sender_user,
			replacement_room,
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
			let event_content = match services
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

			services
				.rooms
				.timeline
				.build_and_append_pdu(
					PduBuilder {
						event_type: event_type.to_string().into(),
						content: event_content,
						state_key: Some(StateKey::from(state_key)),
						..Default::default()
					},
					sender_user,
					replacement_room,
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
			.remove_alias(alias, sender_user)
			.await?;

		services
			.rooms
			.alias
			.set_alias(alias, replacement_room.unwrap(), sender_user)?;
	}

	// Get the old room power levels
	let power_levels_event_content: RoomPowerLevelsEventContent = services
		.rooms
		.state_accessor
		.room_state_get_content(&body.room_id, &StateEventType::RoomPowerLevels, "")
		.await
		.map_err(|_| err!(Database("Found room without m.room.power_levels event.")))?;

	// Setting events_default and invite to the greater of 50 and users_default + 1
	let new_level = max(
		int!(50),
		power_levels_event_content
			.users_default
			.checked_add(int!(1))
			.ok_or_else(|| {
				err!(Request(BadJson("users_default power levels event content is not valid")))
			})?,
	);

	// Modify the power levels in the old room to prevent sending of events and
	// inviting new users
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(StateKey::new(), &RoomPowerLevelsEventContent {
				events_default: new_level,
				invite: new_level,
				..power_levels_event_content
			}),
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.boxed()
		.await?;

	drop(state_lock);

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
				space_id,
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
			replacement_room.unwrap()
		);
		// First, drop the space's child event
		let state_lock = services.rooms.state.mutex.lock(space_id).await;
		debug!("Removing space child event for room {} in space {space_id}", &body.room_id);
		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PduBuilder {
					event_type: StateEventType::SpaceChild.into(),
					content: to_raw_value(&RedactedSpaceChildEventContent {})
						.expect("event is valid, we just created it"),
					state_key: Some(body.room_id.clone().as_str().into()),
					..Default::default()
				},
				sender_user,
				Some(space_id),
				&state_lock,
			)
			.boxed()
			.await
			.ok();
		// Now, add a new child event for the replacement room
		debug!(
			"Adding space child event for room {} in space {space_id}",
			replacement_room.unwrap()
		);
		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PduBuilder {
					event_type: StateEventType::SpaceChild.into(),
					content: to_raw_value(&SpaceChildEventContent {
						via: vec![sender_user.server_name().to_owned()],
						order: child.order,
						suggested: child.suggested,
					})
					.expect("event is valid, we just created it"),
					state_key: Some(replacement_room.unwrap().as_str().into()),
					..Default::default()
				},
				sender_user,
				Some(space_id),
				&state_lock,
			)
			.boxed()
			.await
			.ok();
		debug!(
			"Finished updating space {space_id} child event for room {} to {}",
			&body.room_id,
			replacement_room.unwrap()
		);
		drop(state_lock);
	}

	// Return the replacement room id
	Ok(upgrade_room::v3::Response {
		replacement_room: replacement_room.unwrap().to_owned(),
	})
}
