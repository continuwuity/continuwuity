use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Result, debug_error, err, info,
	matrix::pdu::{PduBuilder, gen_event_id_canonical_json},
};
use futures::{FutureExt, join};
use ruma::{
	OwnedServerName, RoomId, UserId,
	api::{client::membership::invite_user, federation::membership::create_invite},
	events::room::member::{MembershipState, RoomMemberEventContent},
};
use service::Services;

use super::banned_room_check;
use crate::Ruma;

/// # `POST /_matrix/client/r0/rooms/{roomId}/invite`
///
/// Tries to send an invite event into the room.
#[tracing::instrument(skip_all, fields(%client), name = "invite")]
pub(crate) async fn invite_user_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<invite_user::v3::Request>,
) -> Result<invite_user::v3::Response> {
	let sender_user = body.sender_user();
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	if !services.users.is_admin(sender_user).await && services.config.block_non_admin_invites {
		debug_error!(
			"User {sender_user} is not an admin and attempted to send an invite to room {}",
			&body.room_id
		);
		return Err!(Request(Forbidden("Invites are not allowed on this server.")));
	}

	banned_room_check(
		&services,
		sender_user,
		Some(&body.room_id),
		body.room_id.server_name(),
		client,
	)
	.await?;

	match &body.recipient {
		| invite_user::v3::InvitationRecipient::UserId { user_id } => {
			let sender_ignored_recipient = services.users.user_is_ignored(sender_user, user_id);
			let recipient_ignored_by_sender =
				services.users.user_is_ignored(user_id, sender_user);

			let (sender_ignored_recipient, recipient_ignored_by_sender) =
				join!(sender_ignored_recipient, recipient_ignored_by_sender);

			if sender_ignored_recipient {
				return Ok(invite_user::v3::Response {});
			}

			if let Ok(target_user_membership) = services
				.rooms
				.state_accessor
				.get_member(&body.room_id, user_id)
				.await
			{
				if target_user_membership.membership == MembershipState::Ban {
					return Err!(Request(Forbidden("User is banned from this room.")));
				}
			}

			if recipient_ignored_by_sender {
				// silently drop the invite to the recipient if they've been ignored by the
				// sender, pretend it worked
				return Ok(invite_user::v3::Response {});
			}

			invite_helper(
				&services,
				sender_user,
				user_id,
				&body.room_id,
				body.reason.clone(),
				false,
			)
			.boxed()
			.await?;

			Ok(invite_user::v3::Response {})
		},
		| _ => {
			Err!(Request(NotFound("User not found.")))
		},
	}
}

pub(crate) async fn invite_helper(
	services: &Services,
	sender_user: &UserId,
	user_id: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	is_direct: bool,
) -> Result {
	if !services.users.is_admin(sender_user).await && services.config.block_non_admin_invites {
		info!(
			"User {sender_user} is not an admin and attempted to send an invite to room \
			 {room_id}"
		);
		return Err!(Request(Forbidden("Invites are not allowed on this server.")));
	}

	if !services.globals.user_is_local(user_id) {
		let (pdu, pdu_json, invite_room_state) = {
			let state_lock = services.rooms.state.mutex.lock(room_id).await;

			let content = RoomMemberEventContent {
				avatar_url: services.users.avatar_url(user_id).await.ok(),
				is_direct: Some(is_direct),
				reason,
				..RoomMemberEventContent::new(MembershipState::Invite)
			};

			let (pdu, pdu_json) = services
				.rooms
				.timeline
				.create_hash_and_sign_event(
					PduBuilder::state(user_id.to_string(), &content),
					sender_user,
					room_id,
					&state_lock,
				)
				.await?;

			let invite_room_state = services.rooms.state.summary_stripped(&pdu).await;

			drop(state_lock);

			(pdu, pdu_json, invite_room_state)
		};

		let room_version_id = services.rooms.state.get_room_version(room_id).await?;

		let response = services
			.sending
			.send_federation_request(user_id.server_name(), create_invite::v2::Request {
				room_id: room_id.to_owned(),
				event_id: (*pdu.event_id).to_owned(),
				room_version: room_version_id.clone(),
				event: services
					.sending
					.convert_to_outgoing_federation_event(pdu_json.clone())
					.await,
				invite_room_state,
				via: services
					.rooms
					.state_cache
					.servers_route_via(room_id)
					.await
					.ok(),
			})
			.await?;

		// We do not add the event_id field to the pdu here because of signature and
		// hashes checks
		let (event_id, value) = gen_event_id_canonical_json(&response.event, &room_version_id)
			.map_err(|e| {
				err!(Request(BadJson(warn!("Could not convert event to canonical JSON: {e}"))))
			})?;

		if pdu.event_id != event_id {
			return Err!(Request(BadJson(warn!(
				%pdu.event_id, %event_id,
				"Server {} sent event with wrong event ID",
				user_id.server_name()
			))));
		}

		let origin: OwnedServerName = serde_json::from_value(serde_json::to_value(
			value
				.get("origin")
				.ok_or_else(|| err!(Request(BadJson("Event missing origin field."))))?,
		)?)
		.map_err(|e| {
			err!(Request(BadJson(warn!("Origin field in event is not a valid server name: {e}"))))
		})?;

		let pdu_id = services
			.rooms
			.event_handler
			.handle_incoming_pdu(&origin, room_id, &event_id, value, true)
			.boxed()
			.await?
			.ok_or_else(|| {
				err!(Request(InvalidParam("Could not accept incoming PDU as timeline event.")))
			})?;

		return services.sending.send_pdu_room(room_id, &pdu_id).await;
	}

	if !services
		.rooms
		.state_cache
		.is_joined(sender_user, room_id)
		.await
	{
		return Err!(Request(Forbidden(
			"You must be joined in the room you are trying to invite from."
		)));
	}

	let state_lock = services.rooms.state.mutex.lock(room_id).await;

	let content = RoomMemberEventContent {
		displayname: services.users.displayname(user_id).await.ok(),
		avatar_url: services.users.avatar_url(user_id).await.ok(),
		blurhash: services.users.blurhash(user_id).await.ok(),
		is_direct: Some(is_direct),
		reason,
		..RoomMemberEventContent::new(MembershipState::Invite)
	};

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(user_id.to_string(), &content),
			sender_user,
			room_id,
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(())
}
