use std::{borrow::Borrow, collections::BTreeSet};

use futures::{
	Future,
	future::{OptionFuture, join, join3},
};
use ruma::{
	EventId, Int, OwnedUserId, RoomVersionId, UserId,
	events::room::{
		create::RoomCreateEventContent,
		join_rules::{JoinRule, RoomJoinRulesEventContent},
		member::{MembershipState, ThirdPartyInvite},
		power_levels::RoomPowerLevelsEventContent,
		third_party_invite::RoomThirdPartyInviteEventContent,
	},
	int,
	serde::{Base64, Raw},
};
use serde::{
	Deserialize,
	de::{Error as _, IgnoredAny},
};
use serde_json::{from_str as from_json_str, value::RawValue as RawJsonValue};

use super::super::{
	Error, Event, Result, StateEventType, StateKey, TimelineEventType,
	power_levels::{
		deserialize_power_levels, deserialize_power_levels_content_fields,
		deserialize_power_levels_content_invite, deserialize_power_levels_content_redact,
	},
	room_version::RoomVersion,
};
use crate::{
	Pdu, debug, error,
	state_res::event_auth::{auth_events::check_auth_events, create_event::check_room_create},
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

/// For the given event `kind` what are the relevant auth events that are needed
/// to authenticate this `content`.
///
/// # Errors
///
/// This function will return an error if the supplied `content` is not a JSON
/// object.
pub fn auth_types_for_event(
	kind: &TimelineEventType,
	sender: &UserId,
	state_key: Option<&str>,
	content: &RawJsonValue,
	room_version: &RoomVersion,
) -> serde_json::Result<Vec<(StateEventType, StateKey)>> {
	if kind == &TimelineEventType::RoomCreate {
		return Ok(vec![]);
	}

	let mut auth_types = if room_version.room_ids_as_hashes {
		vec![
			(StateEventType::RoomPowerLevels, StateKey::new()),
			(StateEventType::RoomMember, sender.as_str().into()),
		]
	} else {
		vec![
			(StateEventType::RoomPowerLevels, StateKey::new()),
			(StateEventType::RoomMember, sender.as_str().into()),
			(StateEventType::RoomCreate, StateKey::new()),
		]
	};

	if kind == &TimelineEventType::RoomMember {
		#[derive(Deserialize)]
		struct RoomMemberContentFields {
			membership: Option<Raw<MembershipState>>,
			third_party_invite: Option<Raw<ThirdPartyInvite>>,
			join_authorised_via_users_server: Option<Raw<OwnedUserId>>,
		}

		if let Some(state_key) = state_key {
			let content: RoomMemberContentFields = from_json_str(content.get())?;

			if let Some(Ok(membership)) = content.membership.map(|m| m.deserialize()) {
				if [MembershipState::Join, MembershipState::Invite, MembershipState::Knock]
					.contains(&membership)
				{
					let key = (StateEventType::RoomJoinRules, StateKey::new());
					if !auth_types.contains(&key) {
						auth_types.push(key);
					}

					if let Some(Ok(u)) = content
						.join_authorised_via_users_server
						.map(|m| m.deserialize())
					{
						let key = (StateEventType::RoomMember, u.as_str().into());
						if !auth_types.contains(&key) {
							auth_types.push(key);
						}
					}
				}

				let key = (StateEventType::RoomMember, state_key.into());
				if !auth_types.contains(&key) {
					auth_types.push(key);
				}

				if membership == MembershipState::Invite {
					if let Some(Ok(t_id)) = content.third_party_invite.map(|t| t.deserialize()) {
						let key =
							(StateEventType::RoomThirdPartyInvite, t_id.signed.token.into());
						if !auth_types.contains(&key) {
							auth_types.push(key);
						}
					}
				}
			}
		}
	}

	Ok(auth_types)
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
pub async fn iterative_auth_check<E, F, Fut>(
	room_version: &RoomVersion,
	incoming_event: &Pdu,
	current_third_party_invite: Option<&Pdu>,
	fetch_event: impl Fn(&EventId) -> Fut + Send,
	create_event: &Pdu,
) -> Result<bool, Error>
where
	Fut: Future<Output = Result<Option<E>, Error>> + Send,
	E: Event + Send + Sync,
	for<'a> &'a E: Event + Send,
{
	debug!("auth_check beginning");
	let sender = incoming_event.sender();

	// If type is m.room.create:
	if *incoming_event.event_type() == TimelineEventType::RoomCreate {
		debug!("start m.room.create check");
		if let Err(e) = check_room_create(incoming_event) {
			warn!("m.room.create event has been rejected: {}", e);
			return Ok(false);
		}

		debug!("m.room.create event was allowed");
		return Ok(true);
	}

	// TODO: we need to know if events have previously been rejected or soft failed
	// For now, we'll just assume the create_event is valid.
	let create_content = from_json_str::<RoomCreateEventContent>(create_event.content().get())
		.expect("provided create event must be valid");
	let room_version = RoomVersion::new(&create_content.room_version)
		.expect("valid create event must have a valid room version");

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

	// Considering the event's auth_events
	let auth_map = check_auth_events(incoming_event, room_id, &room_version, fetch_event).await;
	if let Err(e) = auth_map {
		warn!("event's auth events are invalid: {}", e);
		return Ok(false);
	}

	// If the content of the m.room.create event in the room state has the property
	// m.federate set to false, and the sender domain of the event does not match
	// the sender domain of the create event, reject.
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

	// Only in room versions 5 and below
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
		warn!("m.room.alias event has no state key");
		return Ok(false);
	}

	// If type is m.room.member
	if *incoming_event.event_type() == TimelineEventType::RoomMember {
		debug!("starting m.room.member check");
		let state_key = match incoming_event.state_key() {
			| None => {
				warn!("no state key in member event");
				return Ok(false);
			},
			| Some(s) => s,
		};

		let content: RoomMemberContentFields = from_json_str(incoming_event.content().get())?;
		if content
			.membership
			.as_ref()
			.and_then(|m| m.deserialize().ok())
			.is_none()
		{
			warn!("no valid membership field found for m.room.member event content");
			return Ok(false);
		}

		let target_user =
			<&UserId>::try_from(state_key).map_err(|e| Error::InvalidPdu(format!("{e}")))?;

		let user_for_join_auth = content
			.join_authorised_via_users_server
			.as_ref()
			.and_then(|u| u.deserialize().ok());

		let user_for_join_auth_event: OptionFuture<_> = user_for_join_auth
			.as_ref()
			.map(|auth_user| fetch_state(&StateEventType::RoomMember, auth_user.as_str()))
			.into();

		let target_user_member_event =
			fetch_state(&StateEventType::RoomMember, target_user.as_str());

		let join_rules_event = fetch_state(&StateEventType::RoomJoinRules, "");

		let (join_rules_event, target_user_member_event, user_for_join_auth_event) =
			join3(join_rules_event, target_user_member_event, user_for_join_auth_event).await;

		let user_for_join_auth_membership = user_for_join_auth_event
			.and_then(|mem| from_json_str::<GetMembership>(mem?.content().get()).ok())
			.map_or(MembershipState::Leave, |mem| mem.membership);

		if !valid_membership_change(
			room_version,
			target_user,
			target_user_member_event.as_ref(),
			sender,
			sender_member_event.as_ref(),
			incoming_event,
			current_third_party_invite,
			power_levels_event.as_ref(),
			join_rules_event.as_ref(),
			user_for_join_auth.as_deref(),
			&user_for_join_auth_membership,
			&room_create_event,
		)? {
			return Ok(false);
		}

		debug!("m.room.member event was allowed");
		return Ok(true);
	}

	// If the sender's current membership state is not join, reject
	#[allow(clippy::manual_let_else)]
	let sender_member_event = match sender_member_event {
		| Some(mem) => mem,
		| None => {
			warn!("sender has no membership event");
			return Ok(false);
		},
	};

	if sender_member_event
		.room_id()
		.expect("we have a room ID for non create events")
		!= expected_room_id
	{
		warn!(
			"room_id of incoming event ({}) does not match that of the m.room.create event ({})",
			sender_member_event
				.room_id()
				.expect("event must have a room ID"),
			expected_room_id
		);
		return Ok(false);
	}

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

	if !matches!(membership_state, MembershipState::Join) {
		warn!(
			%sender,
			?membership_state,
			"sender cannot send events without being joined to the room"
		);
		return Ok(false);
	}

	// If type is m.room.third_party_invite
	let mut sender_power_level = match &power_levels_event {
		| Some(pl) => {
			let content =
				deserialize_power_levels_content_fields(pl.content().get(), room_version)?;
			match content.get_user_power(sender) {
				| Some(level) => *level,
				| _ => content.users_default,
			}
		},
		| _ => {
			// If no power level event found the creator gets 100 everyone else gets 0
			let is_creator = if room_version.use_room_create_sender {
				room_create_event.sender() == sender
			} else {
				#[allow(deprecated)]
				from_json_str::<RoomCreateEventContent>(room_create_event.content().get())
					.is_ok_and(|create| create.creator.unwrap() == *sender)
			};

			if is_creator { int!(100) } else { int!(0) }
		},
	};
	if room_version.explicitly_privilege_room_creators {
		// If the user sent the create event, or is listed in additional_creators, just
		// give them Int::MAX
		if sender == room_create_event.sender()
			|| room_create_content
				.additional_creators
				.as_ref()
				.is_some_and(|creators| {
					creators
						.iter()
						.any(|c| c.deserialize().is_ok_and(|c| c == *sender))
				}) {
			trace!("privileging room creator or additional creator");
			// This user is the room creator or an additional creator, give them max power
			// level
			sender_power_level = Int::MAX;
		}
	}

	// Allow if and only if sender's current power level is greater than
	// or equal to the invite level
	if *incoming_event.event_type() == TimelineEventType::RoomThirdPartyInvite {
		let invite_level = match &power_levels_event {
			| Some(power_levels) =>
				deserialize_power_levels_content_invite(
					power_levels.content().get(),
					room_version,
				)?
				.invite,
			| None => int!(0),
		};

		if sender_power_level < invite_level {
			warn!(
				%sender,
				has=%sender_power_level,
				required=%invite_level,
				"sender cannot send invites in this room"
			);
			return Ok(false);
		}

		debug!("m.room.third_party_invite event was allowed");
		return Ok(true);
	}

	// If the event type's required power level is greater than the sender's power
	// level, reject If the event has a state_key that starts with an @ and does
	// not match the sender, reject.
	if !can_send_event(incoming_event, power_levels_event.as_ref(), sender_power_level) {
		warn!(
			%sender,
			event_type=?incoming_event.kind(),
			"sender cannot send event"
		);
		return Ok(false);
	}

	// If type is m.room.power_levels
	if *incoming_event.event_type() == TimelineEventType::RoomPowerLevels {
		debug!("starting m.room.power_levels check");
		let mut creators = BTreeSet::new();
		if room_version.explicitly_privilege_room_creators {
			creators.insert(create_event.sender().to_owned());
			for creator in room_create_content.additional_creators.iter().flatten() {
				creators.insert(creator.deserialize()?);
			}
		}
		match check_power_levels(
			room_version,
			incoming_event,
			power_levels_event.as_ref(),
			sender_power_level,
			&creators,
		) {
			| Some(required_pwr_lvl) =>
				if !required_pwr_lvl {
					warn!("m.room.power_levels was not allowed");
					return Ok(false);
				},
			| _ => {
				warn!("m.room.power_levels was not allowed");
				return Ok(false);
			},
		}
		debug!("m.room.power_levels event allowed");
	}

	// Room version 3: Redaction events are always accepted (provided the event is
	// allowed by `events` and `events_default` in the power levels). However,
	// servers should not apply or send redaction's to clients until both the
	// redaction event and original event have been seen, and are valid. Servers
	// should only apply redaction's to events where the sender's domains match, or
	// the sender of the redaction has the appropriate permissions per the
	// power levels.

	if room_version.extra_redaction_checks
		&& *incoming_event.event_type() == TimelineEventType::RoomRedaction
	{
		let redact_level = match power_levels_event {
			| Some(pl) =>
				deserialize_power_levels_content_redact(pl.content().get(), room_version)?.redact,
			| None => int!(50),
		};

		if !check_redaction(room_version, incoming_event, sender_power_level, redact_level)? {
			warn!(
				%sender,
				%sender_power_level,
				%redact_level,
				"redaction event was not allowed"
			);
			return Ok(false);
		}
	}

	debug!("allowing event passed all checks");
	Ok(true)
}

fn is_creator<EV>(
	v: &RoomVersion,
	c: &BTreeSet<OwnedUserId>,
	ce: &EV,
	user_id: &UserId,
	have_pls: bool,
) -> bool
where
	EV: Event + Send + Sync,
{
	if v.explicitly_privilege_room_creators {
		c.contains(user_id)
	} else if v.use_room_create_sender && !have_pls {
		ce.sender() == user_id
	} else if !have_pls {
		#[allow(deprecated)]
		let creator = from_json_str::<RoomCreateEventContent>(ce.content().get())
			.unwrap()
			.creator
			.ok_or_else(|| serde_json::Error::missing_field("creator"))
			.unwrap();

		creator == user_id
	} else {
		false
	}
}

// TODO deserializing the member, power, join_rules event contents is done in
// conduit just before this is called. Could they be passed in?
/// Does the user who sent this member event have required power levels to do
/// so.
///
/// * `user` - Information about the membership event and user making the
///   request.
/// * `auth_events` - The set of auth events that relate to a membership event.
///
/// This is generated by calling `auth_types_for_event` with the membership
/// event and the current State.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::cognitive_complexity)]
fn valid_membership_change<E>(
	room_version: &RoomVersion,
	target_user: &UserId,
	target_user_membership_event: Option<&E>,
	sender: &UserId,
	sender_membership_event: Option<&E>,
	current_event: &E,
	current_third_party_invite: Option<&E>,
	power_levels_event: Option<&E>,
	join_rules_event: Option<&E>,
	user_for_join_auth: Option<&UserId>,
	user_for_join_auth_membership: &MembershipState,
	create_room: &E,
) -> Result<bool>
where
	E: Event + Send + Sync,
	for<'a> &'a E: Event + Send,
{
	#[derive(Deserialize)]
	struct GetThirdPartyInvite {
		third_party_invite: Option<Raw<ThirdPartyInvite>>,
	}
	let create_content = from_json_str::<RoomCreateContentFields>(create_room.content().get())?;
	let content = current_event.content();

	let target_membership = from_json_str::<GetMembership>(content.get())?.membership;
	let third_party_invite =
		from_json_str::<GetThirdPartyInvite>(content.get())?.third_party_invite;

	let sender_membership = match &sender_membership_event {
		| Some(pdu) => from_json_str::<GetMembership>(pdu.content().get())?.membership,
		| None => MembershipState::Leave,
	};
	let sender_is_joined = sender_membership == MembershipState::Join;

	let target_user_current_membership = match &target_user_membership_event {
		| Some(pdu) => from_json_str::<GetMembership>(pdu.content().get())?.membership,
		| None => MembershipState::Leave,
	};

	let power_levels: RoomPowerLevelsEventContent = match &power_levels_event {
		| Some(ev) => from_json_str(ev.content().get())?,
		| None => RoomPowerLevelsEventContent::default(),
	};

	let mut sender_power = power_levels
		.users
		.get(sender)
		.or_else(|| sender_is_joined.then_some(&power_levels.users_default));

	let mut target_power = power_levels.users.get(target_user).or_else(|| {
		(target_membership == MembershipState::Join).then_some(&power_levels.users_default)
	});

	let mut creators = BTreeSet::new();
	creators.insert(create_room.sender().to_owned());
	if room_version.explicitly_privilege_room_creators {
		// Explicitly privilege room creators
		// If the sender sent the create event, or in additional_creators, give them
		// Int::MAX. Same case for target.
		if let Some(additional_creators) = &create_content.additional_creators {
			for c in additional_creators {
				if let Ok(c) = c.deserialize() {
					creators.insert(c);
				}
			}
		}
		if creators.contains(sender) {
			sender_power = Some(&Int::MAX);
		}
		if creators.contains(target_user) {
			target_power = Some(&Int::MAX);
		}
	}
	trace!(?creators, "creators for room");

	let join_rules = if let Some(jr) = &join_rules_event {
		from_json_str::<RoomJoinRulesEventContent>(jr.content().get())?.join_rule
	} else {
		JoinRule::Invite
	};

	let power_levels_event_id = power_levels_event.as_ref().map(Event::event_id);
	let sender_membership_event_id = sender_membership_event.as_ref().map(Event::event_id);
	let target_user_membership_event_id =
		target_user_membership_event.as_ref().map(Event::event_id);

	let user_for_join_auth_is_valid = if let Some(user_for_join_auth) = user_for_join_auth {
		// Is the authorised user allowed to invite users into this room
		let (auth_user_pl, invite_level) = if let Some(pl) = &power_levels_event {
			// TODO Refactor all powerlevel parsing
			let invite =
				deserialize_power_levels_content_invite(pl.content().get(), room_version)?.invite;

			let content =
				deserialize_power_levels_content_fields(pl.content().get(), room_version)?;
			let user_pl = match content.get_user_power(user_for_join_auth) {
				| Some(level) => *level,
				| _ => content.users_default,
			};

			(user_pl, invite)
		} else {
			(int!(0), int!(0))
		};
		let user_joined = user_for_join_auth_membership == &MembershipState::Join;
		let okay_power = is_creator(
			room_version,
			&creators,
			create_room,
			user_for_join_auth,
			power_levels_event.as_ref().is_some(),
		) || auth_user_pl >= invite_level;
		trace!(
			%auth_user_pl,
			%auth_user_pl,
			%invite_level,
			%user_joined,
			%okay_power,
			passing=%(user_joined && okay_power),
			"user for join auth is valid check details"
		);
		user_joined && okay_power
	} else {
		// No auth user was given
		trace!("No auth user given for join auth");
		false
	};
	let sender_creator = is_creator(
		room_version,
		&creators,
		create_room,
		sender,
		power_levels_event.as_ref().is_some(),
	);
	let target_creator = is_creator(
		room_version,
		&creators,
		create_room,
		target_user,
		power_levels_event.as_ref().is_some(),
	);

	Ok(match target_membership {
		| MembershipState::Join => {
			trace!("starting target_membership=join check");
			// 1. If the only previous event is an m.room.create and the state_key is the
			//    creator,
			// allow
			let mut prev_events = current_event.prev_events();

			let prev_event_is_create_event = prev_events
				.next()
				.is_some_and(|event_id| event_id.borrow() == create_room.event_id().borrow());
			let no_more_prev_events = prev_events.next().is_none();

			if prev_event_is_create_event && no_more_prev_events {
				trace!(
					%sender,
					target_user = %target_user,
					?sender_creator,
					?target_creator,
					"checking if sender is a room creator for initial membership event"
				);
				let is_creator = sender_creator && target_creator;

				if is_creator {
					debug!("sender is room creator, allowing join");
					return Ok(true);
				}
				trace!("sender is not room creator, proceeding with normal auth checks");
			}
			let membership_allows_join = matches!(
				target_user_current_membership,
				MembershipState::Join | MembershipState::Invite
			);
			if sender != target_user {
				// If the sender does not match state_key, reject.
				warn!(
					%sender,
					target_user = %target_user,
					"sender cannot join on behalf of another user"
				);
				false
			} else if target_user_current_membership == MembershipState::Ban {
				// If the sender is banned, reject.
				warn!(
					%sender,
					membership_event_id = ?target_user_membership_event_id,
					"sender cannot join as they are banned from the room"
				);
				false
			} else {
				match join_rules {
					| JoinRule::Invite =>
						if !membership_allows_join {
							warn!(
								%sender,
								membership_event_id = ?target_user_membership_event_id,
								membership = ?target_user_current_membership,
								"sender cannot join as they are not invited to the invite-only room"
							);
							false
						} else {
							trace!(sender=%sender, "sender is invited to room, allowing join");
							true
						},
					| JoinRule::Knock if !room_version.allow_knocking => {
						warn!("Join rule is knock but room version does not allow knocking");
						false
					},
					| JoinRule::Knock =>
						if !membership_allows_join {
							warn!(
								%sender,
								membership_event_id = ?target_user_membership_event_id,
								membership=?target_user_current_membership,
								"sender cannot join a knock room without being invited or already joined"
							);
							false
						} else {
							trace!(sender=%sender, "sender is invited or already joined to room, allowing join");
							true
						},
					| JoinRule::KnockRestricted(_) if !room_version.knock_restricted_join_rule =>
					{
						warn!(
							"Join rule is knock_restricted but room version does not support it"
						);
						false
					},
					| JoinRule::KnockRestricted(_) => {
						if membership_allows_join || user_for_join_auth_is_valid {
							trace!(
								%sender,
								%membership_allows_join,
								%user_for_join_auth_is_valid,
								"sender is invited, already joined to, or authorised to join the room, allowing join"
							);
							true
						} else {
							warn!(
								%sender,
								membership_event_id = ?target_user_membership_event_id,
								membership=?target_user_current_membership,
								%user_for_join_auth_is_valid,
								?user_for_join_auth,
								"sender cannot join as they are not invited nor already joined to the room, nor was a \
								 valid authorising user given to permit the join"
							);
							false
						}
					},
					| JoinRule::Restricted(_) => {
						if membership_allows_join || user_for_join_auth_is_valid {
							trace!(
								%sender,
								%membership_allows_join,
								%user_for_join_auth_is_valid,
								"sender is invited, already joined to, or authorised to join the room, allowing join"
							);
							true
						} else {
							warn!(
								%sender,
								membership_event_id = ?target_user_membership_event_id,
								membership=?target_user_current_membership,
								%user_for_join_auth_is_valid,
								?user_for_join_auth,
								"sender cannot join as they are not invited nor already joined to the room, nor was a \
								 valid authorising user given to permit the join"
							);
							false
						}
					},
					| JoinRule::Public => {
						trace!(%sender, "join rule is public, allowing join");
						true
					},
					| _ => {
						warn!(
							join_rule=?join_rules,
							"Join rule is unknown, or the rule's conditions were not met"
						);
						false
					},
				}
			}
		},
		| MembershipState::Invite => {
			// If content has third_party_invite key
			trace!("starting target_membership=invite check");
			match third_party_invite.and_then(|i| i.deserialize().ok()) {
				| Some(tp_id) =>
					if target_user_current_membership == MembershipState::Ban {
						warn!(?target_user_membership_event_id, "Can't invite banned user");
						false
					} else {
						let allow = verify_third_party_invite(
							Some(target_user),
							sender,
							&tp_id,
							current_third_party_invite,
						);
						if !allow {
							warn!("Third party invite invalid");
						}
						allow
					},
				| _ =>
					if !sender_is_joined {
						warn!(
							%sender,
							?sender_membership_event_id,
							?sender_membership,
							"sender cannot produce an invite without being joined to the room",
						);
						false
					} else if matches!(
						target_user_current_membership,
						MembershipState::Join | MembershipState::Ban
					) {
						warn!(
							?target_user_membership_event_id,
							?target_user_current_membership,
							"cannot invite a user who is banned or already joined",
						);
						false
					} else {
						let allow = sender_creator
							|| sender_power
								.filter(|&p| p >= &power_levels.invite)
								.is_some();
						if !allow {
							warn!(
								%sender,
								has=?sender_power,
								required=?power_levels.invite,
								"sender does not have enough power to produce invites",
							);
						}
						trace!(
							%sender,
							?sender_membership_event_id,
							?sender_membership,
							?target_user_membership_event_id,
							?target_user_current_membership,
							sender_pl=?sender_power,
							required_pl=?power_levels.invite,
							"allowing invite"
						);
						allow
					},
			}
		},
		| MembershipState::Leave => {
			let can_unban = if target_user_current_membership == MembershipState::Ban {
				sender_creator || sender_power.filter(|&p| p >= &power_levels.ban).is_some()
			} else {
				true
			};
			let can_kick = if !matches!(
				target_user_current_membership,
				MembershipState::Ban | MembershipState::Leave
			) {
				if sender_creator {
					// sender is a creator
					true
				} else if sender_power.filter(|&p| p >= &power_levels.kick).is_none() {
					// sender lacks kick power level
					false
				} else if let Some(sp) = sender_power {
					if let Some(tp) = target_power {
						// sender must have more power than target
						sp > tp
					} else {
						// target has default power level
						true
					}
				} else {
					// sender has default power level
					false
				}
			} else {
				true
			};
			if sender == target_user {
				// self-leave
				// let allow = target_user_current_membership == MembershipState::Join
				// 	|| target_user_current_membership == MembershipState::Invite
				// 	|| target_user_current_membership == MembershipState::Knock;
				let allow = matches!(
					target_user_current_membership,
					MembershipState::Join | MembershipState::Invite | MembershipState::Knock
				);
				if !allow {
					warn!(
						%sender,
						current_membership_event_id=?target_user_membership_event_id,
						current_membership=?target_user_current_membership,
						"sender cannot leave as they are not already knocking on, invited to, or joined to the room"
					);
				}
				trace!(sender=%sender, "allowing leave");
				allow
			} else if !sender_is_joined {
				warn!(
					%sender,
					?sender_membership_event_id,
					"sender cannot kick another user as they are not joined to the room",
				);
				false
			} else if !(can_unban && can_kick) {
				// If the target is banned, only a room creator or someone with ban power
				// level can unban them
				warn!(
					%sender,
					?target_user_membership_event_id,
					?power_levels_event_id,
					"sender lacks the power level required to unban users",
				);
				false
			} else if !can_kick {
				warn!(
					%sender,
					%target_user,
					?target_user_membership_event_id,
					?target_user_current_membership,
					?power_levels_event_id,
					"sender does not have enough power to kick the target",
				);
				false
			} else {
				trace!(
					%sender,
					%target_user,
					?target_user_membership_event_id,
					?target_user_current_membership,
					sender_pl=?sender_power,
					target_pl=?target_power,
					required_pl=?power_levels.kick,
					"allowing kick/unban",
				);
				true
			}
		},
		| MembershipState::Ban =>
			if !sender_is_joined {
				warn!(
					%sender,
					?sender_membership_event_id,
					"sender cannot ban another user as they are not joined to the room",
				);
				false
			} else {
				let allow = sender_creator
					|| (sender_power.filter(|&p| p >= &power_levels.ban).is_some()
						&& target_power < sender_power);
				if !allow {
					warn!(
						%sender,
						%target_user,
						?target_user_membership_event_id,
						?power_levels_event_id,
						"sender does not have enough power to ban the target",
					);
				}
				allow
			},
		| MembershipState::Knock if room_version.allow_knocking => {
			// 1. If the `join_rule` is anything other than `knock` or `knock_restricted`,
			//    reject.
			if !matches!(join_rules, JoinRule::KnockRestricted(_) | JoinRule::Knock) {
				warn!(
					"Join rule is not set to knock or knock_restricted, knocking is not allowed"
				);
				false
			} else if matches!(join_rules, JoinRule::KnockRestricted(_))
				&& !room_version.knock_restricted_join_rule
			{
				// 2. If the `join_rule` is `knock_restricted`, but the room does not support
				//    `knock_restricted`, reject.
				warn!(
					"Join rule is set to knock_restricted but room version does not support \
					 knock_restricted, knocking is not allowed"
				);
				false
			} else if sender != target_user {
				// 3. If `sender` does not match `state_key`, reject.
				warn!(
					%sender,
					%target_user,
					"sender cannot knock on behalf of another user",
				);
				false
			} else if matches!(
				sender_membership,
				MembershipState::Ban | MembershipState::Invite | MembershipState::Join
			) {
				// 4. If the `sender`'s current membership is not `ban`, `invite`, or `join`,
				//    allow.
				// 5. Otherwise, reject.
				warn!(
					?target_user_membership_event_id,
					?sender_membership,
					"Knocking with a membership state of ban, invite or join is invalid",
				);
				false
			} else {
				trace!(%sender, "allowing knock");
				true
			}
		},
		| _ => {
			warn!(
				%sender,
				?target_membership,
				%target_user,
				%target_user_current_membership,
				"Unknown or invalid membership transition {} -> {}",
				target_user_current_membership,
				target_membership
			);
			false
		},
	})
}

/// Is the user allowed to send a specific event based on the rooms power
/// levels.
///
/// Does the event have the correct userId as its state_key if it's not the ""
/// state_key.
fn can_send_event(event: &impl Event, ple: Option<&impl Event>, user_level: Int) -> bool {
	// TODO(hydra): This function does not care about creators!
	let event_type_power_level = get_send_level(event.event_type(), event.state_key(), ple);

	debug!(
		required_level = i64::from(event_type_power_level),
		user_level = i64::from(user_level),
		state_key = ?event.state_key(),
		power_level_event_id = ?ple.map(|e| e.event_id().as_str()),
		"permissions factors",
	);

	if user_level < event_type_power_level {
		return false;
	}

	if event.state_key().is_some_and(|k| k.starts_with('@'))
		&& event.state_key() != Some(event.sender().as_str())
	{
		warn!(
			%user_level,
			required=%event_type_power_level,
			state_key=?event.state_key(),
			sender=%event.sender(),
			"state_key starts with @ but does not match sender",
		);
		return false; // permission required to post in this room
	}

	true
}

/// Confirm that the event sender has the required power levels.
fn check_power_levels(
	room_version: &RoomVersion,
	power_event: &impl Event,
	previous_power_event: Option<&impl Event>,
	user_level: Int,
	creators: &BTreeSet<OwnedUserId>,
) -> Option<bool> {
	match power_event.state_key() {
		| Some("") => {},
		| Some(key) => {
			error!(state_key = key, "m.room.power_levels event has non-empty state key");
			return None;
		},
		| None => {
			error!("check_power_levels requires an m.room.power_levels *state* event argument");
			return None;
		},
	}

	// - If any of the keys users_default, events_default, state_default, ban,
	//   redact, kick, or invite in content are present and not an integer, reject.
	// - If either of the keys events or notifications in content are present and
	//   not a dictionary with values that are integers, reject.
	// - If users key in content is not a dictionary with keys that are valid user
	//   IDs with values that are integers, reject.
	let user_content: RoomPowerLevelsEventContent =
		deserialize_power_levels(power_event.content().get(), room_version)?;

	// Validation of users is done in Ruma, synapse for loops validating user_ids
	// and integers here
	debug!("validation of power event finished");

	#[allow(clippy::manual_let_else)]
	let current_state = match previous_power_event {
		| Some(current_state) => current_state,
		// If there is no previous m.room.power_levels event in the room, allow
		| None => return Some(true),
	};

	let current_content: RoomPowerLevelsEventContent =
		deserialize_power_levels(current_state.content().get(), room_version)?;

	let mut user_levels_to_check = BTreeSet::new();
	let old_list = &current_content.users;
	let user_list = &user_content.users;
	for user in old_list.keys().chain(user_list.keys()) {
		let user: &UserId = user;
		user_levels_to_check.insert(user);
	}

	trace!(set = ?user_levels_to_check, "user levels to check");

	let mut event_levels_to_check = BTreeSet::new();
	let old_list = &current_content.events;
	let new_list = &user_content.events;
	for ev_id in old_list.keys().chain(new_list.keys()) {
		event_levels_to_check.insert(ev_id);
	}

	trace!(set = ?event_levels_to_check, "event levels to check");

	let old_state = &current_content;
	let new_state = &user_content;

	// synapse does not have to split up these checks since we can't combine UserIds
	// and EventTypes we do 2 loops

	// UserId loop
	for user in user_levels_to_check {
		let old_level = old_state.users.get(user);
		let new_level = new_state.users.get(user);
		if new_level.is_some() && creators.contains(user) {
			warn!("creators cannot appear in the users list of m.room.power_levels");
			return Some(false); // cannot alter creator power level
		}
		if old_level.is_some() && new_level.is_some() && old_level == new_level {
			continue;
		}

		// If the current value is equal to the sender's current power level, reject
		if user != power_event.sender() && old_level == Some(&user_level) {
			warn!(
				?old_level,
				?new_level,
				?user,
				%user_level,
				sender=%power_event.sender(),
				"cannot alter the power level of a user with the same power level as sender's own"
			);
			return Some(false); // cannot remove ops level == to own
		}

		// If the current value is higher than the sender's current power level, reject
		// If the new value is higher than the sender's current power level, reject
		let old_level_too_big = old_level > Some(&user_level);
		let new_level_too_big = new_level > Some(&user_level);
		if old_level_too_big {
			warn!(
				?old_level,
				?new_level,
				?user,
				%user_level,
				sender=%power_event.sender(),
				"cannot alter the power level of a user with a higher power level than sender's own"
			);
			return Some(false); // cannot add ops greater than own
		}
		if new_level_too_big {
			warn!(
				?old_level,
				?new_level,
				?user,
				%user_level,
				sender=%power_event.sender(),
				"cannot set the power level of a user to a level higher than sender's own"
			);
			return Some(false); // cannot add ops greater than own
		}
	}

	// EventType loop
	for ev_type in event_levels_to_check {
		let old_level = old_state.events.get(ev_type);
		let new_level = new_state.events.get(ev_type);
		if old_level.is_some() && new_level.is_some() && old_level == new_level {
			continue;
		}

		// If the current value is higher than the sender's current power level, reject
		// If the new value is higher than the sender's current power level, reject
		let old_level_too_big = old_level > Some(&user_level);
		let new_level_too_big = new_level > Some(&user_level);
		if old_level_too_big {
			warn!(
				?old_level,
				?new_level,
				?ev_type,
				%user_level,
				sender=%power_event.sender(),
				"cannot alter the power level of an event with a higher power level than sender's own"
			);
			return Some(false); // cannot add ops greater than own
		}
		if new_level_too_big {
			warn!(
				?old_level,
				?new_level,
				?ev_type,
				%user_level,
				sender=%power_event.sender(),
				"cannot set the power level of an event to a level higher than sender's own"
			);
			return Some(false); // cannot add ops greater than own
		}
	}

	// Notifications, currently there is only @room
	if room_version.limit_notifications_power_levels {
		let old_level = old_state.notifications.room;
		let new_level = new_state.notifications.room;
		if old_level != new_level {
			// If the current value is higher than the sender's current power level, reject
			// If the new value is higher than the sender's current power level, reject
			let old_level_too_big = old_level > user_level;
			let new_level_too_big = new_level > user_level;
			if old_level_too_big || new_level_too_big {
				warn!(
					?old_level,
					?new_level,
					%user_level,
					sender=%power_event.sender(),
					"cannot alter the power level of notifications greater than sender's own"
				);
				return Some(false); // cannot add ops greater than own
			}
		}
	}

	let levels = [
		"users_default",
		"events_default",
		"state_default",
		"ban",
		"redact",
		"kick",
		"invite",
	];
	let old_state = serde_json::to_value(old_state).unwrap();
	let new_state = serde_json::to_value(new_state).unwrap();
	for lvl_name in &levels {
		if let Some((old_lvl, new_lvl)) = get_deserialize_levels(&old_state, &new_state, lvl_name)
		{
			let old_level_too_big = old_lvl > user_level;
			let new_level_too_big = new_lvl > user_level;

			if old_level_too_big || new_level_too_big {
				warn!(
					?old_lvl,
					?new_lvl,
					%user_level,
					sender=%power_event.sender(),
					action=%lvl_name,
					"cannot alter the power level of action greater than sender's own",
				);
				return Some(false);
			}
		}
	}

	Some(true)
}

fn get_deserialize_levels(
	old: &serde_json::Value,
	new: &serde_json::Value,
	name: &str,
) -> Option<(Int, Int)> {
	Some((
		serde_json::from_value(old.get(name)?.clone()).ok()?,
		serde_json::from_value(new.get(name)?.clone()).ok()?,
	))
}

/// Does the event redacting come from a user with enough power to redact the
/// given event.
fn check_redaction(
	_room_version: &RoomVersion,
	redaction_event: &impl Event,
	user_level: Int,
	redact_level: Int,
) -> Result<bool> {
	if user_level >= redact_level {
		debug!("redaction allowed via power levels");
		return Ok(true);
	}

	// If the domain of the event_id of the event being redacted is the same as the
	// domain of the event_id of the m.room.redaction, allow
	if redaction_event.event_id().server_name()
		== redaction_event
			.redacts()
			.as_ref()
			.and_then(|&id| id.server_name())
	{
		debug!("redaction event allowed via room version 1 rules");
		return Ok(true);
	}

	Ok(false)
}

/// Helper function to fetch the power level needed to send an event of type
/// `e_type` based on the rooms "m.room.power_level" event.
fn get_send_level(
	e_type: &TimelineEventType,
	state_key: Option<&str>,
	power_lvl: Option<&impl Event>,
) -> Int {
	power_lvl
		.and_then(|ple| {
			from_json_str::<RoomPowerLevelsEventContent>(ple.content().get())
				.map(|content| {
					content.events.get(e_type).copied().unwrap_or_else(|| {
						if state_key.is_some() {
							content.state_default
						} else {
							content.events_default
						}
					})
				})
				.ok()
		})
		.unwrap_or_else(|| if state_key.is_some() { int!(50) } else { int!(0) })
}

fn verify_third_party_invite(
	target_user: Option<&UserId>,
	sender: &UserId,
	tp_id: &ThirdPartyInvite,
	current_third_party_invite: Option<&impl Event>,
) -> bool {
	// 1. Check for user being banned happens before this is called
	// checking for mxid and token keys is done by ruma when deserializing

	// The state key must match the invitee
	if target_user != Some(&tp_id.signed.mxid) {
		return false;
	}

	// If there is no m.room.third_party_invite event in the current room state with
	// state_key matching token, reject
	#[allow(clippy::manual_let_else)]
	let current_tpid = match current_third_party_invite {
		| Some(id) => id,
		| None => return false,
	};

	if current_tpid.state_key() != Some(&tp_id.signed.token) {
		return false;
	}

	if sender != current_tpid.sender() {
		return false;
	}

	// If any signature in signed matches any public key in the
	// m.room.third_party_invite event, allow
	#[allow(clippy::manual_let_else)]
	let tpid_ev =
		match from_json_str::<RoomThirdPartyInviteEventContent>(current_tpid.content().get()) {
			| Ok(ev) => ev,
			| Err(_) => return false,
		};

	#[allow(clippy::manual_let_else)]
	let decoded_invite_token = match Base64::parse(&tp_id.signed.token) {
		| Ok(tok) => tok,
		// FIXME: Log a warning?
		| Err(_) => return false,
	};

	// A list of public keys in the public_keys field
	for key in tpid_ev.public_keys.unwrap_or_default() {
		if key.public_key == decoded_invite_token {
			return true;
		}
	}

	// A single public key in the public_key field
	tpid_ev.public_key == decoded_invite_token
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
			event_auth::valid_membership_change,
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
