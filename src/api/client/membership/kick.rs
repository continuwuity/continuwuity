use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PartialPdu};
use ruma::{
	api::client::membership::kick_user,
	assign,
	events::room::member::{MembershipState, RoomMemberEventContent},
};

use crate::Ruma;

/// # `POST /_matrix/client/r0/rooms/{roomId}/kick`
///
/// Tries to send a kick event into the room.
pub(crate) async fn kick_user_route(
	State(services): State<crate::State>,
	body: Ruma<kick_user::v3::Request>,
) -> Result<kick_user::v3::Response> {
	let sender_user = body.sender_user();
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}
	let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	if !services
		.rooms
		.state_cache
		.is_joined(&body.user_id, &body.room_id)
		.await
	{
		return Err!(Request(Forbidden("You cannot kick users who are not in the room.")));
	}

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(
				body.user_id.to_string(),
				&assign!(RoomMemberEventContent::new(MembershipState::Leave), {
					reason: body.reason.clone(),
					// TODO(upstream): MSC4293
				}),
			),
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(kick_user::v3::Response::new())
}
