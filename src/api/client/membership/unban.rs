use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PartialPdu};
use ruma::{
	api::client::membership::unban_user,
	events::room::member::{MembershipState, RoomMemberEventContent},
};

use crate::Ruma;

/// # `POST /_matrix/client/r0/rooms/{roomId}/unban`
///
/// Tries to send an unban event into the room.
pub(crate) async fn unban_user_route(
	State(services): State<crate::State>,
	body: Ruma<unban_user::v3::Request>,
) -> Result<unban_user::v3::Response> {
	let sender_user = body.sender_user();
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}
	let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	let mut current_member_content = services
		.rooms
		.state_accessor
		.get_member(&body.room_id, &body.user_id)
		.await
		.unwrap_or_else(|_| RoomMemberEventContent::new(MembershipState::Leave));

	if current_member_content.membership != MembershipState::Ban {
		return Err!(Request(Forbidden(
			"Cannot unban a user who is not banned (current membership: {})",
			current_member_content.membership
		)));
	}

	current_member_content.membership = MembershipState::Leave;
	current_member_content.reason = body.reason.clone();
	current_member_content.join_authorized_via_users_server = None;
	current_member_content.third_party_invite = None;
	current_member_content.is_direct = None;

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(body.user_id.to_string(), &current_member_content),
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(unban_user::v3::Response::new())
}
