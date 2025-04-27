use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PduBuilder};
use ruma::{
	api::client::membership::kick_user,
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
	let state_lock = services.rooms.state.mutex.lock(&body.room_id).await;

	let Ok(event) = services
		.rooms
		.state_accessor
		.get_member(&body.room_id, &body.user_id)
		.await
	else {
		// copy synapse's behaviour of returning 200 without any change to the state
		// instead of erroring on left users
		return Ok(kick_user::v3::Response::new());
	};

	if !matches!(
		event.membership,
		MembershipState::Invite | MembershipState::Knock | MembershipState::Join,
	) {
		return Err!(Request(Forbidden(
			"Cannot kick a user who is not apart of the room (current membership: {})",
			event.membership
		)));
	}

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(body.user_id.to_string(), &RoomMemberEventContent {
				membership: MembershipState::Leave,
				reason: body.reason.clone(),
				is_direct: None,
				join_authorized_via_users_server: None,
				third_party_invite: None,
				..event
			}),
			body.sender_user(),
			&body.room_id,
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(kick_user::v3::Response::new())
}
