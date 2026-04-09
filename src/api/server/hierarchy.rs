use axum::extract::State;
use conduwuit::{Err, Result, info};
use ruma::api::federation::space::get_hierarchy;
use service::rooms::summary::Accessibility;

use crate::Ruma;

/// # `GET /_matrix/federation/v1/hierarchy/{roomId}`
///
/// Gets the space tree in a depth-first manner to locate child rooms of a given
/// space.
pub(crate) async fn get_hierarchy_route(
	State(services): State<crate::State>,
	body: Ruma<get_hierarchy::v1::Request>,
) -> Result<get_hierarchy::v1::Response> {
	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await
	{
		info!(
			origin = body.origin().as_str(),
			"Refusing to serve state for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	let response = services
		.rooms
		.summary
		.get_local_room_summary_for_server(body.origin(), &body.room_id, body.suggested_only)
		.await;

	if let Accessibility::Accessible(response) = response {
		Ok(response)
	} else {
		Err!(Request(NotFound("This room is not accessible.")))
	}
}
