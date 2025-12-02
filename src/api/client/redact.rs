use axum::extract::State;
use conduwuit::{Err, Result, matrix::pdu::PduBuilder};
use ruma::{
	MilliSecondsSinceUnixEpoch, api::client::redact::redact_event,
	events::room::redaction::RoomRedactionEventContent,
};

use crate::Ruma;

/// # `PUT /_matrix/client/r0/rooms/{roomId}/redact/{eventId}/{txnId}`
///
/// Tries to send a redaction event into the room.
///
/// - TODO: Handle txn id
pub(crate) async fn redact_event_route(
	State(services): State<crate::State>,
	body: Ruma<redact_event::v3::Request>,
) -> Result<redact_event::v3::Response> {
	let sender_user = body.sender_user();
	if body.sender_device.is_some() {
		// Increment the "device last active" metadata
		let device_id = body.sender_device();
		let mut device = services
			.users
			.get_device_metadata(sender_user, device_id)
			.await
			.expect("Device metadata should exist for authenticated device");
		device.last_seen_ts = Some(MilliSecondsSinceUnixEpoch::now());
		services
			.users
			.update_device_last_seen(sender_user, device_id, &device)
			.await?;
	}
	let body = &body.body;
	if services.users.is_suspended(sender_user).await? {
		// TODO: Users can redact their own messages while suspended
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let state_lock = services.rooms.state.mutex.lock(&body.room_id).await;

	let event_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder {
				redacts: Some(body.event_id.clone()),
				..PduBuilder::timeline(&RoomRedactionEventContent {
					redacts: Some(body.event_id.clone()),
					reason: body.reason.clone(),
				})
			},
			sender_user,
			Some(&body.room_id),
			&state_lock,
		)
		.await?;

	drop(state_lock);

	Ok(redact_event::v3::Response { event_id })
}
