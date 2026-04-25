use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PartialPdu};
use ruma::{
	api::client::membership::ban_user,
	assign,
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

	let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(
				body.user_id.to_string(),
				&assign!(RoomMemberEventContent::new(MembershipState::Ban), {
					reason: body.reason.clone(),
					redact_events: body.redact_events,
				}),
			),
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(ban_user::v3::Response::new())
}
