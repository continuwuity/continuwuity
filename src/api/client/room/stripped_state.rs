use axum::extract::State;
use conduwuit::{Err, Result};
use ruma::{
	api::client::state::get_stripped_state, assign, events::ALLOWED_STRIPPED_STATE_EVENTS,
};
use service::rooms::summary::{Accessibility, RoomStrippedState};

use crate::{Ruma, router::ClientIdentity};

/// # `POST /_matrix/client/unstable/org.continuwuity.msc4560.stripped_state/{room_id_or_alias}` ([MSC4560])
///
/// Returns a room's stripped state.
///
/// [MSC4560]: https://github.com/matrix-org/matrix-spec-proposals/pull/4560
#[tracing::instrument(skip_all, name = "stripped_state", level = "info")]
pub(crate) async fn get_stripped_state_route(
	State(services): State<crate::State>,
	body: Ruma<get_stripped_state::unstable::Request>,
) -> Result<get_stripped_state::unstable::Response> {
	let user_id = body.identity.as_ref().and_then(ClientIdentity::sender_user);

	if body.via.len() > get_stripped_state::unstable::MAX_VIAS {
		return Err!(Request(TooLarge("Too many vias, please be less indecisive.")));
	}

	if let Some(forbidden_type) = body
		.state
		.iter()
		.find(|event_type| !ALLOWED_STRIPPED_STATE_EVENTS.contains(event_type))
	{
		return Err!(Request(InvalidParam(
			"Event type {forbidden_type} may not be requested as stripped state."
		)));
	}

	let (room_id, _) = services
		.rooms
		.alias
		.resolve_with_servers(&body.room_id_or_alias, Some(body.via.clone()))
		.await?;

	if services.rooms.metadata.is_banned(&room_id).await {
		return Err!(Request(Forbidden("This room is banned on this homeserver.")));
	}

	match services
		.rooms
		.summary
		.get_stripped_state_for_user(
			user_id,
			&room_id,
			&body.via,
			&body.state,
			body.num_joined_members,
		)
		.await
	{
		| Accessibility::Accessible((
			via,
			RoomStrippedState { state, num_joined_members, .. },
		)) => Ok(
			assign!(get_stripped_state::unstable::Response::new(via), { state, num_joined_members }),
		),
		| Accessibility::Inaccessible => {
			Err!(Request(Forbidden("You may not request this room's stripped state.")))
		},
		| Accessibility::NotFound => {
			Err!(Request(NotFound("This room does not exist.")))
		},
	}
}
