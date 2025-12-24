//! Auth checks relevant to the `m.room.member` event specifically.
//!
//! See: https://spec.matrix.org/v1.16/rooms/v12/#authorization-rules

use ruma::{
	EventId, OwnedUserId, RoomId, UserId,
	events::{
		StateEventType,
		room::join_rules::{JoinRule, RoomJoinRulesEventContent},
	},
};

use crate::{
	Event, EventTypeExt, Pdu, RoomVersion,
	matrix::StateKey,
	state_res::{
		Error,
		event_auth::context::{UserPower, get_rank},
	},
};

#[derive(serde::Deserialize)]
struct PartialMembershipObject {
	membership: Option<String>,
	join_authorized_via_users_server: Option<OwnedUserId>,
	third_party_invite: Option<serde_json::Value>,
}

async fn check_join_event<FE, FS>(
	room_version: RoomVersion,
	event: &Pdu,
	membership: &PartialMembershipObject,
	target: &UserId,
	fetch_event: FE,
	fetch_state: FS,
) -> Result<(), Error>
where
	FE: AsyncFn(&EventId) -> Result<Option<Pdu>, Error>,
	FS: AsyncFn((StateEventType, StateKey)) -> Result<Option<Pdu>, Error>,
{
	// 3.1: If the only previous event is an m.room.create and the state_key is the
	// sender of the m.room.create, allow.
	if event.prev_events.len() == 1 {
		let only_prev = fetch_event(&event.prev_events[0]).await?;
		if let Some(prev_event) = only_prev {
			let k = prev_event.event_type().with_state_key("");
			if k.0 == StateEventType::RoomCreate && k.1.as_str() == event.sender().as_str() {
				return Ok(());
			}
		} else {
			return Err(Error::DependencyFailed(
				event.prev_events[0].to_owned(),
				"Previous event not found when checking join event".to_owned(),
			));
		}
	}

	// 3.2: If the sender does not match state_key, reject.
	if event.sender() != target {
		return Err(Error::AuthConditionFailed(
			"m.room.member join event sender does not match state_key".to_owned(),
		));
	}

	let prev_membership = if let Some(ev) =
		fetch_state(StateEventType::RoomMember.with_state_key(target.as_str())).await?
	{
		Some(ev.get_content::<PartialMembershipObject>().map_err(|e| {
			Error::InvalidPdu(format!("Previous m.room.member event has invalid content: {}", e))
		})?)
	} else {
		None
	};
	let join_rule_content =
		if let Some(jr) = fetch_state(StateEventType::RoomJoinRules.with_state_key("")).await? {
			jr.get_content::<RoomJoinRulesEventContent>().map_err(|e| {
				Error::InvalidPdu(format!("m.room.join_rules event has invalid content: {}", e))
			})?
		} else {
			// Default to invite if no join rules event is present.
			RoomJoinRulesEventContent { join_rule: JoinRule::Private }
		};

	// 3.3: If the sender is banned, reject.
	let prev_member = if let Some(prev_content) = &prev_membership {
		if let Some(membership) = &prev_content.membership {
			if membership == "ban" {
				return Err(Error::AuthConditionFailed(
					"m.room.member join event sender is banned".to_owned(),
				));
			}
			membership
		} else {
			"leave"
		}
	} else {
		"leave"
	};

	// 3.4: If the join_rule is invite or knock then allow if membership
	// state is invite or join.
	// 3.5: If the join_rule is restricted or knock_restricted:
	// 3.5.1: If membership state is join or invite, allow.
	match join_rule_content.join_rule {
		| JoinRule::Invite | JoinRule::Knock => {
			if prev_member == "invite" || prev_member == "join" {
				return Ok(());
			}
			Err(Error::AuthConditionFailed(
				"m.room.member join event not invited under invite/knock join rule".to_owned(),
			))
		},
		| JoinRule::Restricted(_) | JoinRule::KnockRestricted(_) => {
			// 3.5.2: If the join_authorised_via_users_server key in content is not a user
			// with sufficient permission to invite other users or is not a joined
			// member of the room, reject.
			if prev_member == "invite" || prev_member == "join" {
				return Ok(());
			}
			let join_authed_by = membership.join_authorized_via_users_server.as_ref();
			if let Some(user_id) = join_authed_by {
				let rank = get_rank(&room_version, &fetch_state, user_id).await?;
				if rank.0 == UserPower::Standard {
					// This user is not a creator, check that they have
					// sufficient power level
					if rank.1 < rank.2.unwrap().invite {
						return Err(Error::InvalidPdu(
							"m.room.member join event join_authorised_via_users_server does not \
							 have sufficient power level to invite"
								.to_owned(),
						));
					}
				}
				// Check that the user is a joined member of the room
				if let Some(state_event) =
					fetch_state(StateEventType::RoomMember.with_state_key(user_id.as_str()))
						.await?
				{
					let state_content = state_event
						.get_content::<PartialMembershipObject>()
						.map_err(|e| {
							Error::InvalidPdu(format!(
								"m.room.member event has invalid content: {}",
								e
							))
						})?;
					if let Some(state_membership) = &state_content.membership {
						if state_membership == "join" {
							return Ok(());
						}
					}
				}
			} else {
				return Err(Error::AuthConditionFailed(
					"m.room.member join event missing join_authorised_via_users_server"
						.to_owned(),
				));
			}

			// 3.5.3: Otherwise, allow
			return Ok(());
		},
		| JoinRule::Public => return Ok(()),
		| _ => Err(Error::AuthConditionFailed(format!(
			"unknown join rule: {:?}",
			join_rule_content.join_rule
		)))?,
	}
}

pub async fn check_member_event<FE, FS>(
	room_version: RoomVersion,
	room_id: &RoomId,
	event: &Pdu,
	fetch_event: FE,
	fetch_state: FS,
) -> Result<(), Error>
where
	FE: AsyncFn(&EventId) -> Result<Option<Pdu>, Error>,
	FS: AsyncFn((StateEventType, StateKey)) -> Result<Option<Pdu>, Error>,
{
	// 1. If there is no state_key property, or no membership property in content,
	//    reject.
	if event.state_key.is_none() {
		return Err(Error::InvalidPdu("m.room.member event missing state_key".to_owned()));
	}

	let target = UserId::parse(event.state_key().unwrap())
		.map_err(|_| Error::InvalidPdu("m.room.member event has invalid state_key".to_owned()))?
		.to_owned();
	let content = event
		.get_content::<PartialMembershipObject>()
		.map_err(|e| {
			Error::InvalidPdu(format!("m.room.member event has invalid content: {}", e))
		})?;

	if content.membership.is_none() {
		return Err(Error::InvalidPdu(
			"m.room.member event missing membership in content".to_owned(),
		));
	}
	let membership = content.membership.as_ref().unwrap();

	// 2: If content has a join_authorised_via_users_server key
	//
	// 2.1: If the event is not validly signed by the homeserver of the user ID
	// denoted by the key, reject.
	if let Some(_join_auth) = &content.join_authorized_via_users_server {
		// We need to check the signature here, but don't have the means to do so yet.
		todo!("Implement join_authorised_via_users_server check");
	}

	// 3: If membership is join:
	if membership == "join" {
		check_join_event(room_version, event, &content, &target, fetch_event, fetch_state)
			.await?;
	}
	Ok(())
}
