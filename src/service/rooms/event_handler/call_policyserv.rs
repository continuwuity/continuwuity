//! Policy server integration for event spam checking in Matrix rooms.
//!
//! This module implements a check against a room-specific policy server, as
//! described in the relevant Matrix spec proposal (see: https://github.com/matrix-org/matrix-spec-proposals/pull/4284).

use std::time::Duration;

use conduwuit::{Err, Event, PduEvent, Result, debug, implement, warn};
use ruma::{
	RoomId, ServerName,
	api::federation::room::policy::v1::Request as PolicyRequest,
	events::{StateEventType, room::policy::RoomPolicyEventContent},
};

/// Returns Ok if the policy server allows the event
#[implement(super::Service)]
#[tracing::instrument(skip_all, level = "debug")]
pub async fn policyserv_check(&self, pdu: &PduEvent, room_id: &RoomId) -> Result {
	if *pdu.event_type() == StateEventType::RoomPolicy.into() {
		debug!(
			room_id = %room_id,
			event_type = ?pdu.event_type(),
			"Skipping spam check for policy server meta-event"
		);
		return Ok(());
	}
	let Ok(policyserver) = self
		.services
		.state_accessor
		.room_state_get_content(room_id, &StateEventType::RoomPolicy, "")
		.await
		.map(|c: RoomPolicyEventContent| c)
	else {
		return Ok(());
	};

	let via = match policyserver.via {
		| Some(ref via) => ServerName::parse(via)?,
		| None => {
			debug!("No policy server configured for room {room_id}");
			return Ok(());
		},
	};
	if via.is_empty() {
		debug!("Policy server is empty for room {room_id}, skipping spam check");
		return Ok(());
	}
	if !self.services.state_cache.server_in_room(via, room_id).await {
		debug!(
			room_id = %room_id,
			via = %via,
			"Policy server is not in the room, skipping spam check"
		);
		return Ok(());
	}
	let outgoing = self
		.services
		.sending
		.convert_to_outgoing_federation_event(pdu.to_canonical_object())
		.await;
	debug!(
		room_id = %room_id,
		via = %via,
		outgoing = ?outgoing,
		"Checking event for spam with policy server"
	);
	let response = tokio::time::timeout(
		Duration::from_secs(10),
		self.services
			.sending
			.send_federation_request(via, PolicyRequest {
				event_id: pdu.event_id().to_owned(),
				pdu: Some(outgoing),
			}),
	)
	.await;
	let response = match response {
		| Ok(Ok(response)) => response,
		| Ok(Err(e)) => {
			warn!(
				via = %via,
				event_id = %pdu.event_id(),
				room_id = %room_id,
				"Failed to contact policy server: {e}"
			);
			// Network or policy server errors are treated as non-fatal: event is allowed by
			// default.
			return Ok(());
		},
		| Err(_) => {
			warn!(
				via = %via,
				event_id = %pdu.event_id(),
				room_id = %room_id,
				"Policy server request timed out after 10 seconds"
			);
			return Ok(());
		},
	};
	if response.recommendation == "spam" {
		warn!(
			via = %via,
			event_id = %pdu.event_id(),
			room_id = %room_id,
			"Event was marked as spam by policy server",
		);
		return Err!(Request(Forbidden("Event was marked as spam by policy server")));
	}

	Ok(())
}
