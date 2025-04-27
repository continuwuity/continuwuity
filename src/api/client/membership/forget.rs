use axum::extract::State;
use conduwuit::{Err, Result, is_matching, result::NotFound, utils::FutureBoolExt};
use futures::pin_mut;
use ruma::{api::client::membership::forget_room, events::room::member::MembershipState};

use crate::Ruma;

/// # `POST /_matrix/client/v3/rooms/{roomId}/forget`
///
/// Forgets about a room.
///
/// - If the sender user currently left the room: Stops sender user from
///   receiving information about the room
///
/// Note: Other devices of the user have no way of knowing the room was
/// forgotten, so this has to be called from every device
pub(crate) async fn forget_room_route(
	State(services): State<crate::State>,
	body: Ruma<forget_room::v3::Request>,
) -> Result<forget_room::v3::Response> {
	let user_id = body.sender_user();
	let room_id = &body.room_id;

	let joined = services.rooms.state_cache.is_joined(user_id, room_id);
	let knocked = services.rooms.state_cache.is_knocked(user_id, room_id);
	let invited = services.rooms.state_cache.is_invited(user_id, room_id);

	pin_mut!(joined, knocked, invited);
	if joined.or(knocked).or(invited).await {
		return Err!(Request(Unknown("You must leave the room before forgetting it")));
	}

	let membership = services
		.rooms
		.state_accessor
		.get_member(room_id, user_id)
		.await;

	if membership.is_not_found() {
		return Err!(Request(Unknown("No membership event was found, room was never joined")));
	}

	let non_membership = membership
		.map(|member| member.membership)
		.is_ok_and(is_matching!(MembershipState::Leave | MembershipState::Ban));

	if non_membership || services.rooms.state_cache.is_left(user_id, room_id).await {
		services.rooms.state_cache.forget(room_id, user_id);
	}

	Ok(forget_room::v3::Response::new())
}
