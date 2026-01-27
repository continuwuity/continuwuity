use std::borrow::ToOwned;

use axum::extract::State;
use conduwuit::{Err, Error, Result, debug, debug_info, info, matrix::pdu::PduBuilder, warn};
use conduwuit_service::Services;
use futures::StreamExt;
use ruma::{
	CanonicalJsonObject, OwnedUserId, RoomId, RoomVersionId, UserId,
	api::{client::error::ErrorKind, federation::membership::prepare_join_event},
	events::{
		StateEventType,
		room::{
			join_rules::{AllowRule, JoinRule, RoomJoinRulesEventContent},
			member::{MembershipState, RoomMemberEventContent},
		},
	},
};
use serde_json::value::to_raw_value;

use crate::Ruma;

/// # `GET /_matrix/federation/v1/make_join/{roomId}/{userId}`
///
/// Creates a join template.
#[tracing::instrument(skip_all, fields(room_id = %body.room_id, user_id = %body.user_id, origin = %body.origin()), level = "info")]
pub(crate) async fn create_join_event_template_route(
	State(services): State<crate::State>,
	body: Ruma<prepare_join_event::v1::Request>,
) -> Result<prepare_join_event::v1::Response> {
	if !services.rooms.metadata.exists(&body.room_id).await {
		return Err!(Request(NotFound("Room is unknown to this server.")));
	}
	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await
	{
		info!(
			origin = body.origin().as_str(),
			"Refusing to serve make_join for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	if body.user_id.server_name() != body.origin() {
		return Err!(Request(BadJson("Not allowed to join on behalf of another server/user.")));
	}

	// ACL check origin server
	services
		.rooms
		.event_handler
		.acl_check(body.origin(), &body.room_id)
		.await?;

	if services
		.moderation
		.is_remote_server_forbidden(body.origin())
	{
		warn!(
			"Server {} for remote user {} tried joining room ID {} which has a server name that \
			 is globally forbidden. Rejecting.",
			body.origin(),
			&body.user_id,
			&body.room_id,
		);
		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			return Err!(Request(Forbidden(warn!(
				"Room ID server name {server} is banned on this homeserver."
			))));
		}
	}

	let room_version_id = services.rooms.state.get_room_version(&body.room_id).await?;
	if !body.ver.contains(&room_version_id) {
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion { room_version: room_version_id },
			"Room version not supported.",
		));
	}

	let state_lock = services.rooms.state.mutex.lock(&body.room_id).await;
	let is_invited = services
		.rooms
		.state_cache
		.is_invited(&body.user_id, &body.room_id)
		.await;
	let join_authorized_via_users_server: Option<OwnedUserId> = {
		use RoomVersionId::*;
		if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6 | V7) || is_invited {
			// room version does not support restricted join rules, or the user is currently
			// already invited
			None
		} else if user_can_perform_restricted_join(
			&services,
			&body.user_id,
			&body.room_id,
			&room_version_id,
		)
		.await?
		{
			let Some(auth_user) = services
				.rooms
				.state_cache
				.local_users_in_room(&body.room_id)
				.filter(|user| {
					services.rooms.state_accessor.user_can_invite(
						&body.room_id,
						user,
						&body.user_id,
						&state_lock,
					)
				})
				.boxed()
				.next()
				.await
				.map(ToOwned::to_owned)
			else {
				info!(
					"No local user is able to authorize the join of {} into {}",
					&body.user_id, &body.room_id
				);
				return Err!(Request(UnableToGrantJoin(
					"No user on this server is able to assist in joining."
				)));
			};
			Some(auth_user)
		} else {
			None
		}
	};
	if services.antispam.check_all_joins() && join_authorized_via_users_server.is_none() {
		if services
			.antispam
			.meowlnir_accept_make_join(body.room_id.clone(), body.user_id.clone())
			.await
			.is_err()
		{
			return Err!(Request(Forbidden("Antispam rejected join request.")));
		}
	}

	let (_pdu, mut pdu_json) = services
		.rooms
		.timeline
		.create_hash_and_sign_event(
			PduBuilder::state(body.user_id.to_string(), &RoomMemberEventContent {
				join_authorized_via_users_server,
				..RoomMemberEventContent::new(MembershipState::Join)
			}),
			&body.user_id,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;
	drop(state_lock);

	// room v3 and above removed the "event_id" field from remote PDU format
	maybe_strip_event_id(&mut pdu_json, &room_version_id)?;

	Ok(prepare_join_event::v1::Response {
		room_version: Some(room_version_id),
		event: to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON"),
	})
}

/// Checks whether the given user can join the given room via a restricted join.
pub(crate) async fn user_can_perform_restricted_join(
	services: &Services,
	user_id: &UserId,
	room_id: &RoomId,
	room_version_id: &RoomVersionId,
) -> Result<bool> {
	use RoomVersionId::*;

	// restricted rooms are not supported on <=v7
	if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6 | V7) {
		return Ok(false);
	}

	if services.rooms.state_cache.is_joined(user_id, room_id).await {
		// joining user is already joined, there is nothing we need to do
		return Ok(false);
	}

	let Ok(join_rules_event_content) = services
		.rooms
		.state_accessor
		.room_state_get_content::<RoomJoinRulesEventContent>(
			room_id,
			&StateEventType::RoomJoinRules,
			"",
		)
		.await
	else {
		// No join rules means there's nothing to authorise (defaults to invite)
		return Ok(false);
	};

	let (JoinRule::Restricted(r) | JoinRule::KnockRestricted(r)) =
		join_rules_event_content.join_rule
	else {
		return Ok(false);
	};

	if r.allow.is_empty() {
		debug_info!("{room_id} is restricted but the allow key is empty");
		return Ok(false);
	}

	for allow_rule in &r.allow {
		match allow_rule {
			| AllowRule::RoomMembership(membership) => {
				if services
					.rooms
					.state_cache
					.is_joined(user_id, &membership.room_id)
					.await
				{
					debug!(
						"User {} is allowed to join room {} via membership in room {}",
						user_id, room_id, membership.room_id
					);
					return Ok(true);
				}
			},
			| AllowRule::UnstableSpamChecker =>
				return match services
					.antispam
					.meowlnir_accept_make_join(room_id.to_owned(), user_id.to_owned())
					.await
				{
					| Ok(()) => Ok(true),
					| Err(_) => Err!(Request(Forbidden("Antispam rejected join request."))),
				},
			| _ => {
				debug_info!(
					"Unsupported allow rule in restricted join for room {}: {:?}",
					room_id,
					allow_rule
				);
			},
		}
	}

	Err!(Request(UnableToAuthorizeJoin(
		"Joining user is not known to be in any required room."
	)))
}

pub(crate) fn maybe_strip_event_id(
	pdu_json: &mut CanonicalJsonObject,
	room_version_id: &RoomVersionId,
) -> Result {
	use RoomVersionId::*;

	match room_version_id {
		| V1 | V2 => Ok(()),
		| _ => {
			pdu_json.remove("event_id");
			Ok(())
		},
	}
}
