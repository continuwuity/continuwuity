use axum::extract::State;
use conduwuit::{Err, Result};
use futures::StreamExt;
use ruma::api::client::membership::mutual_rooms;

use crate::Ruma;

/// # `GET /_matrix/client/unstable/uk.half-shot.msc2666/user/mutual_rooms`
///
/// Gets all the rooms the sender shares with the specified user.
///
/// An implementation of [MSC2666](https://github.com/matrix-org/matrix-spec-proposals/pull/2666)
#[tracing::instrument(skip_all, name = "mutual_rooms", level = "info")]
pub(crate) async fn get_mutual_rooms_route(
	State(services): State<crate::State>,
	body: Ruma<mutual_rooms::unstable::Request>,
) -> Result<mutual_rooms::unstable::Response> {
	let sender_user = body.sender_user();

	if sender_user == body.user_id {
		return Err!(Request(Unknown("You cannot request rooms in common with yourself.")));
	}

	if !services.users.exists(&body.user_id).await {
		return Ok(mutual_rooms::unstable::Response::new(vec![]));
	}

	let mutual_rooms = services
		.rooms
		.state_cache
		.get_shared_rooms(sender_user, &body.user_id)
		.collect()
		.await;

	Ok(mutual_rooms::unstable::Response::new(mutual_rooms))
}
