//! Auth checks relevant to the `m.room.member` event specifically.
//!
//! See: https://spec.matrix.org/v1.16/rooms/v12/#authorization-rules

use ruma::{
	EventId, OwnedUserId, UserId,
	events::{
		StateEventType,
		room::{
			join_rules::{JoinRule, RoomJoinRulesEventContent},
			third_party_invite::{PublicKey, RoomThirdPartyInviteEventContent},
		},
	},
	serde::Base64,
	signatures::{PublicKeyMap, PublicKeySet, verify_json},
};
use serde::Deserializer;

use crate::{
	Event, EventTypeExt, Pdu, RoomVersion,
	matrix::StateKey,
	state_res::{
		Error,
		event_auth::context::{UserPower, get_rank},
	},
	utils::to_canonical_object,
};

#[derive(serde::Deserialize, Default)]
struct PartialMembershipObject {
	membership: Option<String>,
	join_authorized_via_users_server: Option<OwnedUserId>,
	third_party_invite: Option<serde_json::Value>,
}

/// Fetches the membership *content* of the target.
/// If there is not one, an empty leave membership is returned.
async fn fetch_membership<FS>(
	fetch_state: &FS,
	target: &UserId,
) -> Result<PartialMembershipObject, Error>
where
	FS: AsyncFn((StateEventType, StateKey)) -> Result<Option<Pdu>, Error>,
{
	fetch_state(StateEventType::RoomMember.with_state_key(target.as_str()))
		.await
		.map(|pdu| {
			if let Some(ev) = pdu {
				ev.get_content::<PartialMembershipObject>().map_err(|e| {
					Error::InvalidPdu(format!("m.room.member event has invalid content: {}", e))
				})
			} else {
				Ok(PartialMembershipObject {
					membership: Some("leave".to_owned()),
					..Default::default()
				})
			}
		})?
}

async fn check_join_event<FE, FS>(
	room_version: &RoomVersion,
	event: &Pdu,
	membership: &PartialMembershipObject,
	target: &UserId,
	fetch_event: &FE,
	fetch_state: &FS,
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
				let rank = get_rank(&room_version, fetch_state, user_id).await?;
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

async fn check_invite_event<FE, FS>(
	room_version: &RoomVersion,
	event: &Pdu,
	membership: &PartialMembershipObject,
	target: &UserId,
	fetch_state: &FS,
) -> Result<(), Error>
where
	FE: AsyncFn(&EventId) -> Result<Option<Pdu>, Error>,
	FS: AsyncFn((StateEventType, StateKey)) -> Result<Option<Pdu>, Error>,
{
	let target_current_membership = fetch_membership(fetch_state, target).await?;

	// 4.1: If content has a third_party_invite property:
	if let Some(raw_third_party_invite) = &membership.third_party_invite {
		// 4.1.1: If target user is banned, reject.
		if target_current_membership
			.membership
			.is_some_and(|m| m == "ban")
		{
			return Err(Error::AuthConditionFailed("invite target is banned".to_owned()));
		}
		// 4.1.2: If content.third_party_invite does not have a signed property, reject.
		let signed = raw_third_party_invite.get("signed").ok_or_else(|| {
			Error::AuthConditionFailed(
				"invite event third_party_invite missing signed property".to_owned(),
			)
		})?;
		// 4.2.3: If signed does not have mxid and token properties, reject.
		let mxid = signed.get("mxid").and_then(|v| v.as_str()).ok_or_else(|| {
			Error::AuthConditionFailed(
				"invite event third_party_invite signed missing/invalid mxid property".to_owned(),
			)
		})?;
		let token = signed
			.get("token")
			.and_then(|v| v.as_str())
			.ok_or_else(|| {
				Error::AuthConditionFailed(
					"invite event third_party_invite signed missing token property".to_owned(),
				)
			})?;
		// 4.2.4: If mxid does not match state_key, reject.
		if mxid != target.as_str() {
			return Err(Error::AuthConditionFailed(
				"invite event third_party_invite signed mxid does not match state_key".to_owned(),
			));
		}
		// 4.2.5: If there is no m.room.third_party_invite event in the room
		// state matching the token, reject.
		let Some(third_party_invite_event) =
			fetch_state(StateEventType::RoomThirdPartyInvite.with_state_key(token)).await?
		else {
			return Err(Error::AuthConditionFailed(
				"invite event third_party_invite token has no matching m.room.third_party_invite"
					.to_owned(),
			));
		};
		// 4.2.6: If sender does not match sender of the m.room.third_party_invite,
		// reject.
		if third_party_invite_event.sender() != event.sender() {
			return Err(Error::AuthConditionFailed(
				"invite event sender does not match m.room.third_party_invite sender".to_owned(),
			));
		}
		// 4.2.7: If any signature in signed matches any public key in the
		// m.room.third_party_invite event, allow. The public keys are in
		// content of m.room.third_party_invite as:
		//   1. A single public key in the public_key property.
		//   2. A list of public keys in the public_keys property.
		let tpi_content = third_party_invite_event
			.get_content::<RoomThirdPartyInviteEventContent>()
			.or_else(|_| {
				Err(Error::InvalidPdu(
					"m.room.third_party_invite event has invalid content".to_owned(),
				))
			})?;
		let mut public_keys = tpi_content.public_keys.unwrap_or_default();
		public_keys.push(PublicKey {
			public_key: tpi_content.public_key,
			key_validity_url: None,
		});

		let signatures = signed
			.get("signatures")
			.and_then(|v| v.as_object())
			.ok_or_else(|| {
				Error::InvalidPdu(
					"invite event third_party_invite signed missing/invalid signatures"
						.to_owned(),
				)
			})?;
		let mut public_key_map = PublicKeyMap::new();
		for (server_name, sig_map) in signatures {
			let mut pk_set = PublicKeySet::new();
			if let Some(sig_map) = sig_map.as_object() {
				for (key_id, sig) in sig_map {
					let sig_b64 = Base64::parse(sig.as_str().ok_or(Error::InvalidPdu(
						"invite event third_party_invite signature is not a string".to_owned(),
					))?)
					.map_err(|_| {
						Error::InvalidPdu(
							"invite event third_party_invite signature is not valid Base64"
								.to_owned(),
						)
					})?;
					pk_set.insert(key_id.clone(), sig_b64);
				}
			}
			public_key_map.insert(server_name.clone(), pk_set);
		}
		verify_json(
			&public_key_map,
			to_canonical_object(signed).expect("signed was already validated"),
		)
		.map_err(|e| {
			Error::AuthConditionFailed(format!(
				"invite event third_party_invite signature verification failed: {e}"
			))
		})?;
		// If there was no error, there was a valid signature, so allow.
		return Ok(());
	}

	// 4.2: If the sender’s current membership state is not join, reject.
	let sender_membership = fetch_membership(fetch_state, event.sender()).await?;
	if sender_membership.membership.is_none_or(|m| m != "join") {
		return Err(Error::AuthConditionFailed("invite sender is not joined".to_owned()));
	}

	// 4.3: If target user’s current membership state is join or ban, reject.
	if target_current_membership
		.membership
		.is_some_and(|m| m == "join" || m == "ban")
	{
		return Err(Error::AuthConditionFailed(
			"invite target is already joined or banned".to_owned(),
		));
	}

	// 4.4: If the sender’s power level is greater than or equal to the invite
	// level, allow.
	let (rank, pl, pl_evt) = get_rank(&room_version, fetch_state, event.sender()).await?;
	if rank == UserPower::Creator || pl >= pl_evt.unwrap_or_default().invite {
		return Ok(());
	}

	// 4.5: Otherwise, reject.
	Err(Error::AuthConditionFailed(
		"invite sender does not have sufficient power level to invite".to_owned(),
	))
}

pub async fn check_member_event<FE, FS>(
	room_version: RoomVersion,
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

	// 2: If content has a join_authorised_via_users_server key
	//
	// 2.1: If the event is not validly signed by the homeserver of the user ID
	// denoted by the key, reject.
	if let Some(_join_auth) = &content.join_authorized_via_users_server {
		// We need to check the signature here, but don't have the means to do so yet.
		todo!("Implement join_authorised_via_users_server check");
	}

	match content.membership.as_deref().unwrap() {
		| "join" =>
			check_join_event(&room_version, event, &content, &target, &fetch_event, &fetch_state)
				.await?,
		| "invite" =>
			check_invite_event(&room_version, event, &content, &target, &fetch_state).await?,
		| _ => {
			todo!()
		},
	};
	Ok(())
}
