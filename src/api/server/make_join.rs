use axum::extract::State;
use conduwuit::{Err, Error, Result, info, matrix::pdu::PartialPdu, utils};
use ruma::{
	OwnedUserId,
	api::{
		error::{ErrorKind, IncompatibleRoomVersionErrorData},
		federation::membership::prepare_join_event,
	},
	assign,
	events::room::{
		join_rules::JoinRule,
		member::{MembershipState, RoomMemberEventContent},
	},
};
use serde_json::value::to_raw_value;
use service::rooms::membership::JoinConditionResult;

use crate::Ruma;

/// # `GET /_matrix/federation/v1/make_join/{roomId}/{userId}`
///
/// Creates a join template.
#[tracing::instrument(skip_all, fields(room_id = %body.room_id, user_id = %body.user_id, origin = %body.identity), level = "info")]
pub(crate) async fn create_join_event_template_route(
	State(services): State<crate::State>,
	body: Ruma<prepare_join_event::v1::Request>,
) -> Result<prepare_join_event::v1::Response> {
	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await
	{
		info!(
			origin = body.identity.as_str(),
			room_id = %body.room_id,
			"Refusing to serve make_join for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	if body.user_id.server_name() != body.identity {
		return Err!(Request(BadJson("Not allowed to join on behalf of another server/user.")));
	}

	// ACL check origin server
	services
		.rooms
		.event_handler
		.acl_check(&body.identity, &body.room_id)
		.await?;

	let room_version = services.rooms.state.get_room_version(&body.room_id).await?;
	let room_version_rules = room_version.rules().unwrap();
	if !body.ver.contains(&room_version) {
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData::new(
				room_version,
			)),
			"Room version not supported.",
		));
	}

	let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	let current_membership = services
		.rooms
		.state_accessor
		.get_member(&body.room_id, &body.user_id)
		.await
		.ok()
		.map_or_else(|| MembershipState::Leave, |content| content.membership);

	let join_authorized_via_users_server: Option<OwnedUserId> = {
		if matches!(
			current_membership,
			MembershipState::Join | MembershipState::Invite | MembershipState::Knock
		) {
			// User is already joined, invited, or knocked, and consequently
			// does not need an authorising user
			None
		} else if !room_version_rules.authorization.restricted_join_rule {
			// Room version does not support restricted join rules
			None
		} else {
			let join_rules = services
				.rooms
				.state_accessor
				.get_join_rules(&body.room_id)
				.await;

			if let JoinRule::Restricted(conditions) | JoinRule::KnockRestricted(conditions) =
				join_rules
			{
				match services
					.rooms
					.membership
					.evaluate_join_conditions(&body.room_id, &body.user_id, &conditions)
					.await
				{
					| JoinConditionResult::Satisfied => {
						// Try to find an authorizing user
						services
							.rooms
							.membership
							.select_authorizing_user(&body.room_id, &body.user_id, &state_lock)
							.await
							.map(Some)?
					},
					| JoinConditionResult::Unsatisfied => {
						return Err!(Request(Forbidden(
							"You do not satisfy this room's join conditions."
						)));
					},
					| JoinConditionResult::UnableToAuthorizeJoin => {
						return Err!(Request(UnableToAuthorizeJoin(
							"This server cannot check this room's join conditions."
						)));
					},
				}
			} else {
				None
			}
		}
	};

	if services.antispam.check_all_joins() {
		if services
			.antispam
			.meowlnir_accept_make_join(body.room_id.clone(), body.user_id.clone())
			.await
			!= JoinConditionResult::Satisfied
		{
			return Err!(Request(Forbidden("Antispam rejected join request.")));
		}
	}

	let (pdu, _) = services
		.rooms
		.timeline
		.create_event(
			PartialPdu::state(
				body.user_id.to_string(),
				&assign!(RoomMemberEventContent::new(MembershipState::Join), {
					join_authorized_via_users_server,
				}),
			),
			&body.user_id,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;
	drop(state_lock);
	let mut pdu_json = utils::to_canonical_object(&pdu)
		.expect("Barebones PDU should be convertible to canonical JSON");
	pdu_json.remove("event_id");

	Ok(
		assign!(prepare_join_event::v1::Response::new(to_raw_value(&pdu_json).expect("CanonicalJson can be serialized to JSON")), {
			room_version: Some(room_version),
		}),
	)
}
