use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PartialPdu};
use ruma::{api::client::membership::kick_user, events::room::member::MembershipState};

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

	let Ok(mut event) = services
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

	event.membership = MembershipState::Leave;
	event.reason.clone_from(&body.reason);
	event.is_direct = None;
	event.join_authorized_via_users_server = None;
	event.third_party_invite = None;

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(body.user_id.to_string(), &event),
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(kick_user::v3::Response::new())
}
