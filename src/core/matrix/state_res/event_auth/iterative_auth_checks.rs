use ruma::{
	EventId, OwnedUserId, RoomVersionId,
	events::{
		StateEventType, TimelineEventType,
		room::{create::RoomCreateEventContent, member::MembershipState},
	},
	int,
	serde::Raw,
};
use serde::{Deserialize, de::IgnoredAny};
use serde_json::from_str as from_json_str;

use crate::{
	Event, EventTypeExt, Pdu, RoomVersion, debug, error,
	matrix::StateKey,
	state_res::{
		error::Error,
		event_auth::{
			auth_events::check_auth_events,
			context::{UserPower, calculate_creators, get_rank},
			create_event::check_room_create,
			member_event::check_member_event,
			power_levels::check_power_levels,
		},
	},
	trace, warn,
};

// FIXME: field extracting could be bundled for `content`
#[derive(Deserialize)]
struct GetMembership {
	membership: MembershipState,
}

#[derive(Deserialize, Debug)]
struct RoomMemberContentFields {
	membership: Option<Raw<MembershipState>>,
	join_authorised_via_users_server: Option<Raw<OwnedUserId>>,
}

#[derive(Deserialize)]
struct RoomCreateContentFields {
	room_version: Option<Raw<RoomVersionId>>,
	creator: Option<Raw<IgnoredAny>>,
	additional_creators: Option<Vec<Raw<OwnedUserId>>>,
	#[serde(rename = "m.federate", default = "ruma::serde::default_true")]
	federate: bool,
}

/// Authenticate the incoming `event`.
///
/// The steps of authentication are:
///
/// * check that the event is being authenticated for the correct room
/// * then there are checks for specific event types
///
/// The `fetch_state` closure should gather state from a state snapshot. We need
/// to know if the event passes auth against some state not a recursive
/// collection of auth_events fields.
#[tracing::instrument(
	skip_all,
	fields(
		event_id = incoming_event.event_id().as_str(),
		event_type = ?incoming_event.event_type().to_string()
	)
)]
#[allow(clippy::suspicious_operation_groupings)]
pub async fn auth_check<FE, FS>(
	room_version: &RoomVersion,
	incoming_event: &Pdu,
	fetch_event: &FE,
	fetch_state: &FS,
	create_event: Option<&Pdu>,
) -> Result<bool, Error>
where
	FE: AsyncFn(&EventId) -> Result<Option<Pdu>, Error>,
	FS: AsyncFn((StateEventType, StateKey)) -> Result<Option<Pdu>, Error>,
{
	debug!("auth_check beginning");
	let sender = incoming_event.sender();

	// Since v1, If type is m.room.create:
	if *incoming_event.event_type() == TimelineEventType::RoomCreate {
		debug!("start m.room.create check");
		if let Err(e) = check_room_create(incoming_event) {
			warn!("m.room.create event has been rejected: {}", e);
			return Ok(false);
		}

		debug!("m.room.create event was allowed");
		return Ok(true);
	}
	let Some(create_event) = create_event else {
		error!("no create event provided for auth check");
		return Err(Error::InvalidPdu("missing create event".to_owned()));
	};

	// TODO: we need to know if events have previously been rejected or soft failed
	// For now, we'll just assume the create_event is valid.
	let create_content = from_json_str::<RoomCreateEventContent>(create_event.content().get())
		.expect("provided create event must be valid");

	// Since v12, If the event’s room_id is not an event ID for an accepted (not
	// rejected) m.room.create event, with the sigil ! instead of $, reject.
	if room_version.room_ids_as_hashes {
		let calculated_room_id = create_event.event_id().as_str().replace('$', "!");
		if let Some(claimed_room_id) = create_event.room_id() {
			if claimed_room_id.as_str() != calculated_room_id {
				warn!(
					expected = %calculated_room_id,
					received = %claimed_room_id,
					"event's room ID does not match the hash of the m.room.create event ID"
				);
				return Ok(false);
			}
		} else {
			warn!("event is missing a room ID");
			return Ok(false);
		}
	}

	let room_id = incoming_event.room_id().expect("event must have a room ID");

	let auth_map =
		match check_auth_events(incoming_event, room_id, &room_version, fetch_event).await {
			| Ok(map) => map,
			| Err(e) => {
				warn!("event's auth events are invalid: {}", e);
				return Ok(false);
			},
		};

	// Considering the event's auth_events

	// Since v1, If the content of the m.room.create event in the room state has the
	// property m.federate set to false, and the sender domain of the event does
	// not match the sender domain of the create event, reject.
	if !create_content.federate {
		if create_event.sender().server_name() != incoming_event.sender().server_name() {
			warn!(
				sender = %incoming_event.sender(),
				create_sender = %create_event.sender(),
				"room is not federated and event's sender domain does not match create event's sender domain"
			);
			return Ok(false);
		}
	}

	// From v1 to v5, If type is m.room.aliases
	if room_version.special_case_aliases_auth
		&& *incoming_event.event_type() == TimelineEventType::RoomAliases
	{
		if let Some(state_key) = incoming_event.state_key() {
			// If sender's domain doesn't matches state_key, reject
			if state_key != sender.server_name().as_str() {
				warn!("state_key does not match sender");
				return Ok(false);
			}
			// Otherwise, allow
			return Ok(true);
		}
		// If event has no state_key, reject.
		warn!("m.room.alias event has no state key");
		return Ok(false);
	}

	// From v1, If type is m.room.member
	if *incoming_event.event_type() == TimelineEventType::RoomMember {
		if let Err(e) =
			check_member_event(&room_version, incoming_event, fetch_event, fetch_state).await
		{
			warn!("m.room.member event has been rejected: {}", e);
			return Ok(false);
		}
	}

	// From v1, If the sender's current membership state is not join, reject
	let sender_member_event =
		match auth_map.get(&StateEventType::RoomMember.with_state_key(sender.as_str())) {
			| Some(ev) => ev,
			| None => {
				warn!(
					%sender,
					"sender is not joined - no membership event found for sender in auth events"
				);
				return Ok(false);
			},
		};

	let sender_membership_event_content: RoomMemberContentFields =
		from_json_str(sender_member_event.content().get())?;
	let Some(membership_state) = sender_membership_event_content.membership else {
		warn!(
			?sender_membership_event_content,
			"Sender membership event content missing membership field"
		);
		return Err(Error::InvalidPdu("Missing membership field".to_owned()));
	};
	let membership_state = membership_state.deserialize()?;

	if membership_state != MembershipState::Join {
		warn!(
			%sender,
			?membership_state,
			"sender cannot send events without being joined to the room"
		);
		return Ok(false);
	}

	// From v1, If type is m.room.third_party_invite
	let (rank, sender_pl, pl_evt) = get_rank(&room_version, fetch_state, sender).await?;

	// Allow if and only if sender's current power level is greater than
	// or equal to the invite level
	if *incoming_event.event_type() == TimelineEventType::RoomThirdPartyInvite {
		if rank == UserPower::Creator {
			trace!("sender is room creator, allowing m.room.third_party_invite");
			return Ok(true);
		}
		let invite_level = match &pl_evt {
			| Some(power_levels) => power_levels.invite,
			| None => int!(0),
		};

		if sender_pl < invite_level {
			warn!(
				%sender,
				has=%sender_pl,
				required=%invite_level,
				"sender cannot send invites in this room"
			);
			return Ok(false);
		}

		debug!("m.room.third_party_invite event was allowed");
		return Ok(true);
	}

	// Since v1, if the event type’s required power level is greater than the
	// sender’s power level, reject.
	let required_level = match &pl_evt {
		| Some(power_levels) => power_levels
			.events
			.get(incoming_event.kind())
			.unwrap_or_else(|| {
				if incoming_event.state_key.is_some() {
					&power_levels.state_default
				} else {
					&power_levels.events_default
				}
			}),
		| None => &int!(0),
	};
	if rank != UserPower::Creator && sender_pl < *required_level {
		warn!(
			%sender,
			has=%sender_pl,
			required=%required_level,
			"sender does not have enough power level to send this event"
		);
		return Ok(false);
	}

	// Since v1, If the event has a state_key that starts with an @ and does not
	// match the sender, reject.
	if let Some(state_key) = incoming_event.state_key() {
		if state_key.starts_with('@') && state_key != sender.as_str() {
			warn!(
				%sender,
				%state_key,
				"event's state key starts with @ and does not match sender"
			);
			return Ok(false);
		}
	}

	// Since v1, If type is m.room.power_levels
	if *incoming_event.event_type() == TimelineEventType::RoomPowerLevels {
		let creators = calculate_creators(&room_version, fetch_state).await?;
		if let Err(e) =
			check_power_levels(&room_version, incoming_event, pl_evt.as_ref(), creators).await
		{
			warn!(
				%sender,
				"m.room.power_levels event has been rejected: {}", e
			);
			return Ok(false);
		}
	}

	// From v1 to v2: If type is m.room.redaction:
	// If the sender’s power level is greater than or equal to the redact level,
	// allow.
	// If the domain of the event_id of the event being redacted is the same as the
	// domain of the event_id of the m.room.redaction, allow.
	// Otherwise, reject.
	if room_version.extra_redaction_checks {
		// We'll panic here, since while we don't theoretically support the room
		// versions that require this, we don't want to incorrectly permit an event
		// that should be rejected in this theoretically impossible scenario.
		unreachable!(
			"continuwuity does not support room versions that require extra redaction checks"
		);
	}

	debug!("allowing event passed all checks");
	Ok(true)
}

#[cfg(test)]
mod tests {
	use ruma::events::{
		StateEventType, TimelineEventType,
		room::{
			join_rules::{
				AllowRule, JoinRule, Restricted, RoomJoinRulesEventContent, RoomMembership,
			},
			member::{MembershipState, RoomMemberEventContent},
		},
	};
	use serde_json::value::to_raw_value as to_raw_json_value;

	use crate::{
		matrix::{Event, EventTypeExt, Pdu as PduEvent},
		state_res::{
			RoomVersion, StateMap,
			event_auth::{
				iterative_auth_checks::valid_membership_change, valid_membership_change,
			},
			test_utils::{
				INITIAL_EVENTS, INITIAL_EVENTS_CREATE_ROOM, alice, charlie, ella, event_id,
				member_content_ban, member_content_join, room_id, to_pdu_event,
			},
		},
	};

	#[test]
	fn test_ban_pass() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let events = INITIAL_EVENTS();

		let auth_events = events
			.values()
			.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.clone()))
			.collect::<StateMap<_>>();

		let requester = to_pdu_event(
			"HELLO",
			alice(),
			TimelineEventType::RoomMember,
			Some(charlie().as_str()),
			member_content_ban(),
			&[],
			&["IMC"],
		);

		let fetch_state = |ty, key| auth_events.get(&(ty, key)).cloned();
		let target_user = charlie();
		let sender = alice();

		assert!(
			valid_membership_change(
				&RoomVersion::V6,
				target_user,
				fetch_state(StateEventType::RoomMember, target_user.as_str().into()).as_ref(),
				sender,
				fetch_state(StateEventType::RoomMember, sender.as_str().into()).as_ref(),
				&requester,
				None::<&PduEvent>,
				fetch_state(StateEventType::RoomPowerLevels, "".into()).as_ref(),
				fetch_state(StateEventType::RoomJoinRules, "".into()).as_ref(),
				None,
				&MembershipState::Leave,
				&fetch_state(StateEventType::RoomCreate, "".into()).unwrap(),
			)
			.unwrap()
		);
	}

	#[test]
	fn test_join_non_creator() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let events = INITIAL_EVENTS_CREATE_ROOM();

		let auth_events = events
			.values()
			.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.clone()))
			.collect::<StateMap<_>>();

		let requester = to_pdu_event(
			"HELLO",
			charlie(),
			TimelineEventType::RoomMember,
			Some(charlie().as_str()),
			member_content_join(),
			&["CREATE"],
			&["CREATE"],
		);

		let fetch_state = |ty, key| auth_events.get(&(ty, key)).cloned();
		let target_user = charlie();
		let sender = charlie();

		assert!(
			!valid_membership_change(
				&RoomVersion::V6,
				target_user,
				fetch_state(StateEventType::RoomMember, target_user.as_str().into()).as_ref(),
				sender,
				fetch_state(StateEventType::RoomMember, sender.as_str().into()).as_ref(),
				&requester,
				None::<&PduEvent>,
				fetch_state(StateEventType::RoomPowerLevels, "".into()).as_ref(),
				fetch_state(StateEventType::RoomJoinRules, "".into()).as_ref(),
				None,
				&MembershipState::Leave,
				&fetch_state(StateEventType::RoomCreate, "".into()).unwrap(),
			)
			.unwrap()
		);
	}

	#[test]
	fn test_join_creator() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let events = INITIAL_EVENTS_CREATE_ROOM();

		let auth_events = events
			.values()
			.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.clone()))
			.collect::<StateMap<_>>();

		let requester = to_pdu_event(
			"HELLO",
			alice(),
			TimelineEventType::RoomMember,
			Some(alice().as_str()),
			member_content_join(),
			&["CREATE"],
			&["CREATE"],
		);

		let fetch_state = |ty, key| auth_events.get(&(ty, key)).cloned();
		let target_user = alice();
		let sender = alice();

		assert!(
			valid_membership_change(
				&RoomVersion::V6,
				target_user,
				fetch_state(StateEventType::RoomMember, target_user.as_str().into()).as_ref(),
				sender,
				fetch_state(StateEventType::RoomMember, sender.as_str().into()).as_ref(),
				&requester,
				None::<&PduEvent>,
				fetch_state(StateEventType::RoomPowerLevels, "".into()).as_ref(),
				fetch_state(StateEventType::RoomJoinRules, "".into()).as_ref(),
				None,
				&MembershipState::Leave,
				&fetch_state(StateEventType::RoomCreate, "".into()).unwrap(),
			)
			.unwrap()
		);
	}

	#[test]
	fn test_ban_fail() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let events = INITIAL_EVENTS();

		let auth_events = events
			.values()
			.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.clone()))
			.collect::<StateMap<_>>();

		let requester = to_pdu_event(
			"HELLO",
			charlie(),
			TimelineEventType::RoomMember,
			Some(alice().as_str()),
			member_content_ban(),
			&[],
			&["IMC"],
		);

		let fetch_state = |ty, key| auth_events.get(&(ty, key)).cloned();
		let target_user = alice();
		let sender = charlie();

		assert!(
			!valid_membership_change(
				&RoomVersion::V6,
				target_user,
				fetch_state(StateEventType::RoomMember, target_user.as_str().into()).as_ref(),
				sender,
				fetch_state(StateEventType::RoomMember, sender.as_str().into()).as_ref(),
				&requester,
				None::<&PduEvent>,
				fetch_state(StateEventType::RoomPowerLevels, "".into()).as_ref(),
				fetch_state(StateEventType::RoomJoinRules, "".into()).as_ref(),
				None,
				&MembershipState::Leave,
				&fetch_state(StateEventType::RoomCreate, "".into()).unwrap(),
			)
			.unwrap()
		);
	}

	#[test]
	fn test_restricted_join_rule() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let mut events = INITIAL_EVENTS();
		*events.get_mut(&event_id("IJR")).unwrap() = to_pdu_event(
			"IJR",
			alice(),
			TimelineEventType::RoomJoinRules,
			Some(""),
			to_raw_json_value(&RoomJoinRulesEventContent::new(JoinRule::Restricted(
				Restricted::new(vec![AllowRule::RoomMembership(RoomMembership::new(
					room_id().to_owned(),
				))]),
			)))
			.unwrap(),
			&["CREATE", "IMA", "IPOWER"],
			&["IPOWER"],
		);

		let mut member = RoomMemberEventContent::new(MembershipState::Join);
		member.join_authorized_via_users_server = Some(alice().to_owned());

		let auth_events = events
			.values()
			.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.clone()))
			.collect::<StateMap<_>>();

		let requester = to_pdu_event(
			"HELLO",
			ella(),
			TimelineEventType::RoomMember,
			Some(ella().as_str()),
			to_raw_json_value(&RoomMemberEventContent::new(MembershipState::Join)).unwrap(),
			&["CREATE", "IJR", "IPOWER", "new"],
			&["new"],
		);

		let fetch_state = |ty, key| auth_events.get(&(ty, key)).cloned();
		let target_user = ella();
		let sender = ella();

		assert!(
			valid_membership_change(
				&RoomVersion::V9,
				target_user,
				fetch_state(StateEventType::RoomMember, target_user.as_str().into()).as_ref(),
				sender,
				fetch_state(StateEventType::RoomMember, sender.as_str().into()).as_ref(),
				&requester,
				None::<&PduEvent>,
				fetch_state(StateEventType::RoomPowerLevels, "".into()).as_ref(),
				fetch_state(StateEventType::RoomJoinRules, "".into()).as_ref(),
				Some(alice()),
				&MembershipState::Join,
				&fetch_state(StateEventType::RoomCreate, "".into()).unwrap(),
			)
			.unwrap()
		);

		assert!(
			!valid_membership_change(
				&RoomVersion::V9,
				target_user,
				fetch_state(StateEventType::RoomMember, target_user.as_str().into()).as_ref(),
				sender,
				fetch_state(StateEventType::RoomMember, sender.as_str().into()).as_ref(),
				&requester,
				None::<&PduEvent>,
				fetch_state(StateEventType::RoomPowerLevels, "".into()).as_ref(),
				fetch_state(StateEventType::RoomJoinRules, "".into()).as_ref(),
				Some(ella()),
				&MembershipState::Leave,
				&fetch_state(StateEventType::RoomCreate, "".into()).unwrap(),
			)
			.unwrap()
		);
	}

	#[test]
	fn test_knock() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let mut events = INITIAL_EVENTS();
		*events.get_mut(&event_id("IJR")).unwrap() = to_pdu_event(
			"IJR",
			alice(),
			TimelineEventType::RoomJoinRules,
			Some(""),
			to_raw_json_value(&RoomJoinRulesEventContent::new(JoinRule::Knock)).unwrap(),
			&["CREATE", "IMA", "IPOWER"],
			&["IPOWER"],
		);

		let auth_events = events
			.values()
			.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.clone()))
			.collect::<StateMap<_>>();

		let requester = to_pdu_event(
			"HELLO",
			ella(),
			TimelineEventType::RoomMember,
			Some(ella().as_str()),
			to_raw_json_value(&RoomMemberEventContent::new(MembershipState::Knock)).unwrap(),
			&[],
			&["IMC"],
		);

		let fetch_state = |ty, key| auth_events.get(&(ty, key)).cloned();
		let target_user = ella();
		let sender = ella();

		assert!(
			valid_membership_change(
				&RoomVersion::V7,
				target_user,
				fetch_state(StateEventType::RoomMember, target_user.as_str().into()).as_ref(),
				sender,
				fetch_state(StateEventType::RoomMember, sender.as_str().into()).as_ref(),
				&requester,
				None::<&PduEvent>,
				fetch_state(StateEventType::RoomPowerLevels, "".into()).as_ref(),
				fetch_state(StateEventType::RoomJoinRules, "".into()).as_ref(),
				None,
				&MembershipState::Leave,
				&fetch_state(StateEventType::RoomCreate, "".into()).unwrap(),
			)
			.unwrap()
		);
	}
}
