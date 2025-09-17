use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PduBuilder};
use ruma::{
	api::client::membership::ban_user,
	events::room::member::{MembershipState, RoomMemberEventContent},
};

use crate::Ruma;

/// # `POST /_matrix/client/r0/rooms/{roomId}/ban`
///
/// Tries to send a ban event into the room.
pub(crate) async fn ban_user_route(
	State(services): State<crate::State>,
	body: Ruma<ban_user::v3::Request>,
) -> Result<ban_user::v3::Response> {
	let sender_user = body.sender_user();

	if sender_user == body.user_id {
		return Err!(Request(Forbidden("You cannot ban yourself.")));
	}

	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let state_lock = services.rooms.state.mutex.lock(&body.room_id).await;

	let current_member_content = services
		.rooms
		.state_accessor
		.get_member(&body.room_id, &body.user_id)
		.await
		.unwrap_or_else(|_| RoomMemberEventContent::new(MembershipState::Ban));

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(body.user_id.to_string(), &RoomMemberEventContent {
				membership: MembershipState::Ban,
				reason: body.reason.clone(),
				displayname: None, // display name may be offensive
				avatar_url: None,  // avatar may be offensive
				is_direct: None,
				join_authorized_via_users_server: None,
				third_party_invite: None,
				redact_events: body.redact_events,
				..current_member_content
			}),
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(ban_user::v3::Response::new())
}
