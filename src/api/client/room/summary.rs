use axum::extract::State;
use axum_client_ip::ClientIp;
use conduwuit::{Err, Result};
use ruma::api::client::room::get_summary;
use service::rooms::summary::Accessibility;

use crate::Ruma;

/// # `GET /_matrix/client/v1/room_summary/{roomIdOrAlias}`
///
/// Returns a short description of the state of a room.
#[tracing::instrument(skip_all, fields(%client), name = "room_summary", level = "info")]
pub(crate) async fn get_room_summary(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<get_summary::v1::Request>,
) -> Result<get_summary::v1::Response> {
	let (room_id, servers) = services
		.rooms
		.alias
		.resolve_with_servers(&body.room_id_or_alias, Some(body.via.clone()))
		.await?;

	if services.rooms.metadata.is_banned(&room_id).await {
		return Err!(Request(Forbidden("This room is banned on this homeserver.")));
	}

	let summary = services
		.rooms
		.summary
		.get_room_summary_for_user(body.sender_user.as_deref(), &room_id, &servers)
		.await?;

	match summary {
		| Accessibility::Accessible(summary) => Ok(get_summary::v1::Response::new(summary)),
		| Accessibility::Inaccessible => {
			Err!(Request(Forbidden("You may not preview this room."), FORBIDDEN))
		},
		| Accessibility::NotFound => {
			Err!(Request(Forbidden("This room does not exist."), FORBIDDEN))
		},
	}
}
