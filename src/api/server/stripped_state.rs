use axum::extract::State;
use conduwuit::{Err, Result};
use ruma::{
	api::federation::event::get_stripped_state, assign, events::ALLOWED_STRIPPED_STATE_EVENTS,
};
use service::rooms::summary::{Accessibility, RoomStrippedState};

use crate::Ruma;

/// # `POST /_matrix/federation/unstable/org.continuwuity.msc4560.stripped_state/{room_id}` ([MSC4560])
///
/// Returns a room's stripped state.
///
/// [MSC4560]: https://github.com/matrix-org/matrix-spec-proposals/pull/4560
pub(crate) async fn get_stripped_state_route(
	State(services): State<crate::State>,
	body: Ruma<get_stripped_state::unstable::Request>,
) -> Result<get_stripped_state::unstable::Response> {
	if let Some(forbidden_type) = body
		.state
		.iter()
		.find(|event_type| !ALLOWED_STRIPPED_STATE_EVENTS.contains(event_type))
	{
		return Err!(Request(InvalidParam(
			"Event type {forbidden_type} may not be requested as stripped state."
		)));
	}

	if services.rooms.metadata.is_banned(&body.room_id).await {
		return Err!(Request(Forbidden("This room is banned on this homeserver.")));
	}

	match services
		.rooms
		.summary
		.get_local_stripped_state_for_server(
			&body.identity,
			&body.room_id,
			&body.state,
			body.num_joined_members,
		)
		.await
	{
		| Accessibility::Accessible(RoomStrippedState { state, num_joined_members, .. }) => Ok(
			assign!(get_stripped_state::unstable::Response::default(), { state, num_joined_members }),
		),
		| Accessibility::Inaccessible => {
			Err!(Request(Forbidden("You may not access this room's stripped state.")))
		},
		| Accessibility::NotFound => {
			Err!(Request(NotFound("This room is not known to this server.")))
		},
	}
}
