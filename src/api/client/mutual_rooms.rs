use axum::extract::State;
use conduwuit::{Err, Result};
use futures::StreamExt;
use ruma::{OwnedRoomId, api::client::membership::mutual_rooms};

use crate::Ruma;

/// # `GET /_matrix/client/unstable/uk.half-shot.msc2666/user/mutual_rooms`
///
/// Gets all the rooms the sender shares with the specified user.
///
/// An implementation of [MSC2666](https://github.com/matrix-org/matrix-spec-proposals/pull/2666)
#[tracing::instrument(skip_all, name = "mutual_rooms", level = "info")]
pub(crate) async fn get_mutual_rooms_unstable_route(
	State(services): State<crate::State>,
	body: Ruma<mutual_rooms::unstable::Request>,
) -> Result<mutual_rooms::unstable::Response> {
	let sender_user = body.identity.expect_sender_user()?;

	if sender_user == body.user_id {
		return Err!(Request(InvalidParam("You cannot request rooms in common with yourself.")));
	}

	let mutual_rooms = services
		.rooms
		.state_cache
		.get_shared_rooms(sender_user, &body.user_id)
		.collect()
		.await;

	Ok(mutual_rooms::unstable::Response::new(mutual_rooms))
}

/// # `GET /_matrix/client/v1/mutual_rooms`
///
/// Gets all the rooms the sender shares with the specified user.
#[tracing::instrument(skip_all, name = "mutual_rooms", level = "info")]
pub(crate) async fn get_mutual_rooms_route(
	State(services): State<crate::State>,
	body: Ruma<mutual_rooms::v1::Request>,
) -> Result<mutual_rooms::v1::Response> {
	let sender_user = body.identity.expect_sender_user()?;

	if sender_user == body.user_id {
		return Err!(Request(InvalidParam("You cannot request rooms in common with yourself.")));
	}

	let mutual_rooms: Vec<OwnedRoomId> = services
		.rooms
		.state_cache
		.get_shared_rooms(sender_user, &body.user_id)
		.collect()
		.await;

	Ok(mutual_rooms::v1::Response::new(
		mutual_rooms
			.len()
			.try_into()
			.expect("user should be in fewer than 9.1 quadrillion rooms"),
		mutual_rooms,
	))
}
