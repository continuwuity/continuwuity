//! Auth checks relevant to any event's `auth_events`.
//!
//! See: https://spec.matrix.org/v1.16/rooms/v12/#authorization-rules
use std::collections::{HashMap, HashSet};

use ruma::{
	EventId, OwnedEventId, RoomId, UserId,
	events::{
		StateEventType, TimelineEventType,
		room::member::{MembershipState, RoomMemberEventContent, ThirdPartyInvite},
	},
};

use crate::{Event, EventTypeExt, Pdu, RoomVersion, matrix::StateKey, state_res::Error, warn};

/// For the given event `kind` what are the relevant auth events that are needed
/// to authenticate this `content`.
///
/// # Errors
///
/// This function will return an error if the supplied `content` is not a JSON
/// object.
pub fn auth_types_for_event(
	room_version: &RoomVersion,
	event_type: &TimelineEventType,
	state_key: Option<&StateKey>,
	sender: &UserId,
	member_content: Option<RoomMemberEventContent>,
) -> serde_json::Result<Vec<(StateEventType, StateKey)>> {
	if event_type == &TimelineEventType::RoomCreate {
		// Create events never have auth events
		return Ok(vec![]);
	}
	let mut auth_types = if room_version.room_ids_as_hashes {
		vec![
			StateEventType::RoomMember.with_state_key(sender.as_str()),
			StateEventType::RoomPowerLevels.with_state_key(""),
		]
	} else {
		// For room versions that do not use room IDs as hashes, include the
		// RoomCreate event as an auth event.
		vec![
			StateEventType::RoomMember.with_state_key(sender.as_str()),
			StateEventType::RoomPowerLevels.with_state_key(""),
			StateEventType::RoomCreate.with_state_key(""),
		]
	};

	if event_type == &TimelineEventType::RoomMember {
		let member_content =
			member_content.expect("member_content must be provided for RoomMember events");

		// Include the target's membership (if available)
		auth_types.push((
			StateEventType::RoomMember,
			state_key
				.expect("state_key must be provided for RoomMember events")
				.to_owned(),
		));

		if matches!(
			member_content.membership,
			MembershipState::Join | MembershipState::Invite | MembershipState::Knock
		) {
			// Include the join rules
			auth_types.push(StateEventType::RoomJoinRules.with_state_key(""));
		}

		if matches!(member_content.membership, MembershipState::Invite) {
			// If this is an invite, include the third party invite if it exists
			if let Some(ThirdPartyInvite { signed, .. }) = member_content.third_party_invite {
				auth_types
					.push(StateEventType::RoomThirdPartyInvite.with_state_key(signed.token));
			}
		}

		if matches!(member_content.membership, MembershipState::Join)
			&& room_version.restricted_join_rules
		{
			// If this is a restricted join, include the authorizing user's membership
			if let Some(authorizing_user) = member_content.join_authorized_via_users_server {
				auth_types
					.push(StateEventType::RoomMember.with_state_key(authorizing_user.as_str()));
			}
		}
	}

	Ok(auth_types)
}

/// Checks for duplicate auth events in the `auth_events` field of an event.
/// Note: the caller should already have all of the auth events fetched.
///
/// If there are multiple auth events of the same type and state key, this
/// returns an error. Otherwise, it returns a map of (type, state_key) to the
/// corresponding auth event.
pub async fn check_duplicate_auth_events<FE>(
	auth_events: &[OwnedEventId],
	fetch_event: FE,
) -> Result<HashMap<(StateEventType, StateKey), Pdu>, Error>
where
	FE: AsyncFn(&EventId) -> Result<Option<Pdu>, Error>,
{
	let mut seen: HashMap<(StateEventType, StateKey), Pdu> = HashMap::new();

	// Considering all of the event's auth events:
	for auth_event_id in auth_events {
		if let Ok(Some(auth_event)) = fetch_event(auth_event_id).await {
			let event_type = auth_event.kind();
			// If this is not a state event, reject it.
			let Some(state_key) = &auth_event.state_key() else {
				return Err(Error::InvalidPdu(format!(
					"Auth event {:?} is not a state event",
					auth_event_id
				)));
			};
			let type_key_pair: (StateEventType, StateKey) =
				event_type.clone().with_state_key(state_key.clone());

			// If there are duplicate entries for a given type and state_key pair, reject.
			if seen.contains_key(&type_key_pair) {
				return Err(Error::DuplicateAuthEvents(format!(
					"({:?},\"{:?}\")",
					event_type, state_key
				)));
			}
			seen.insert(type_key_pair, auth_event);
		} else {
			return Err(Error::NotFound(auth_event_id.as_str().to_owned()));
		}
	}

	Ok(seen)
}

// Checks that the event does not refer to any auth events that it does not need
// to.
pub fn check_unnecessary_auth_events(
	auth_events: &HashSet<(StateEventType, StateKey)>,
	expected: &Vec<(StateEventType, StateKey)>,
) -> Result<(), Error> {
	// If there are entries whose type and state_key don't match those specified by
	// the auth events selection algorithm described in the server specification,
	// reject.
	let remaining = auth_events
		.iter()
		.filter(|key| !expected.contains(key))
		.collect::<HashSet<_>>();
	if !remaining.is_empty() {
		return Err(Error::UnselectedAuthEvents(format!("{:?}", remaining)));
	}
	Ok(())
}

// Checks that all provided auth events were not rejected previously.
//
// TODO: this is currently a no-op and always returns Ok(()).
pub fn check_all_auth_events_accepted(
	_auth_events: &HashMap<(StateEventType, StateKey), Pdu>,
) -> Result<(), Error> {
	Ok(())
}

// Checks that all auth events are from the same room as the event being
// validated.
pub fn check_auth_same_room(auth_events: &Vec<Pdu>, room_id: &RoomId) -> bool {
	for auth_event in auth_events {
		if let Some(auth_room_id) = &auth_event.room_id() {
			if auth_room_id.as_str() != room_id.as_str() {
				warn!(
					auth_event_id=%auth_event.event_id(),
					"Auth event room id {} does not match expected room id {}",
					auth_room_id,
					room_id
				);
				return false;
			}
		} else {
			warn!(auth_event_id=%auth_event.event_id(), "Auth event has no room_id");
			return false;
		}
	}
	true
}

/// Performs all auth event checks for the given event.
pub async fn check_auth_events<FE>(
	event: &Pdu,
	room_id: &RoomId,
	room_version: &RoomVersion,
	fetch_event: &FE,
) -> Result<HashMap<(StateEventType, StateKey), Pdu>, Error>
where
	FE: AsyncFn(&EventId) -> Result<Option<Pdu>, Error>,
{
	// If there are duplicate entries for a given type and state_key pair, reject.
	let auth_events_map = check_duplicate_auth_events(&event.auth_events, fetch_event).await?;
	let auth_events_set: HashSet<(StateEventType, StateKey)> =
		auth_events_map.keys().cloned().collect();

	// If there are entries whose type and state_key don’t match those specified by
	// the auth events selection algorithm described in the server specification,
	// reject.
	let member_event_content = match event.kind() {
		| TimelineEventType::RoomMember =>
			Some(event.get_content::<RoomMemberEventContent>().map_err(|e| {
				Error::InvalidPdu(format!("Failed to parse m.room.member content: {}", e))
			})?),
		| _ => None,
	};
	let expected_auth_events = auth_types_for_event(
		room_version,
		event.kind(),
		event.state_key.as_ref(),
		event.sender(),
		member_event_content,
	)?;
	if let Err(e) = check_unnecessary_auth_events(&auth_events_set, &expected_auth_events) {
		return Err(e);
	}

	// If there are entries which were themselves rejected under the checks
	// performed on receipt of a PDU, reject.
	if let Err(e) = check_all_auth_events_accepted(&auth_events_map) {
		return Err(e);
	}

	// If any event in auth_events has a room_id which does not match that of the
	// event being authorised, reject.
	let auth_event_refs: Vec<Pdu> = auth_events_map.values().cloned().collect();
	if !check_auth_same_room(&auth_event_refs, room_id) {
		return Err(Error::InvalidPdu(
			"One or more auth events are from a different room".to_owned(),
		));
	}

	Ok(auth_events_map)
}
