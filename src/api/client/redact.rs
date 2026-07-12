use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PartialPdu};
use ruma::{
	api::client::redact::redact_event, assign, events::room::redaction::RoomRedactionEventContent,
};

use crate::{Ruma, client_ip::ClientIp};

/// # `PUT /_matrix/client/r0/rooms/{roomId}/redact/{eventId}/{txnId}`
///
/// Tries to send a redaction event into the room.
///
/// - TODO: Handle txn id
pub(crate) async fn redact_event_route(
	State(services): State<crate::State>,
	ClientIp(client_ip): ClientIp, // NOTE: required for updating device metadata
	body: Ruma<redact_event::v3::Request>,
) -> Result<redact_event::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	services
		.users
		.update_device_last_seen(sender_user, body.identity.sender_device(), client_ip)
		.await;
	let body = &body.body;
	if services.users.is_suspended(sender_user).await? {
		// TODO: Users can redact their own messages while suspended
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let state_lock = services.rooms.state.mutex.lock(body.room_id.as_str()).await;

	let event_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu {
				redacts: Some(body.event_id.clone()),
				..PartialPdu::timeline(
					&assign!(RoomRedactionEventContent::new_v11(body.event_id.clone()), {
						reason: body.reason.clone()
					}),
				)
			},
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(redact_event::v3::Response::new(event_id))
}
