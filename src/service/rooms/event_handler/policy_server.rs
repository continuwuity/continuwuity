//! Policy server integration for event spam checking in Matrix rooms.
//!
//! This module implements a check against a room-specific policy server, as
//! described in the relevant Matrix spec proposal (see: https://github.com/matrix-org/matrix-spec-proposals/pull/4284).

use std::time::Duration;

use conduwuit::{Err, Event, PduEvent, Result, debug, debug_info, implement, trace, warn};
use ruma::{
	CanonicalJsonObject, RoomId, ServerName,
	api::federation::room::policy::v1::Request as PolicyRequest,
	events::{StateEventType, room::policy::RoomPolicyEventContent},
};

/// Asks a remote policy server if the event is allowed.
///
/// If the event is the `org.matrix.msc4284.policy` configuration state event,
/// this check is skipped. Similarly, if there is no policy server configured in
/// the PDU's room, or the configured server is not present in the room, the
/// check is also skipped.
///
/// If the policy server marks the event as spam, Ok(false) is returned,
/// otherwise Ok(true) allows the event. If the policy server cannot be
/// contacted for whatever reason, Err(e) is returned, which generally is a
/// fail-open operation.
#[implement(super::Service)]
#[tracing::instrument(skip_all, level = "debug")]
pub async fn ask_policy_server(
	&self,
	pdu: &PduEvent,
	pdu_json: &CanonicalJsonObject,
	room_id: &RoomId,
) -> Result<bool> {
	if !self.services.server.config.enable_msc4284_policy_servers {
		return Ok(true); // don't ever contact policy servers
	}
	if self.services.server.config.policy_server_check_own_events
		&& pdu.origin.is_some()
		&& self
			.services
			.server
			.is_ours(pdu.origin.as_ref().unwrap().as_str())
	{
		return Ok(true); // don't contact policy servers for locally generated events
	}

	if *pdu.event_type() == StateEventType::RoomPolicy.into() {
		debug!(
			room_id = %room_id,
			event_type = ?pdu.event_type(),
			"Skipping spam check for policy server meta-event"
		);
		return Ok(true);
	}
	let Ok(policyserver) = self
		.services
		.state_accessor
		.room_state_get_content(room_id, &StateEventType::RoomPolicy, "")
		.await
		.map(|c: RoomPolicyEventContent| c)
	else {
		return Ok(true);
	};

	let via = match policyserver.via {
		| Some(ref via) => ServerName::parse(via)?,
		| None => {
			trace!("No policy server configured for room {room_id}");
			return Ok(true);
		},
	};
	if via.is_empty() {
		trace!("Policy server is empty for room {room_id}, skipping spam check");
		return Ok(true);
	}
	if !self.services.state_cache.server_in_room(via, room_id).await {
		debug!(
			room_id = %room_id,
			via = %via,
			"Policy server is not in the room, skipping spam check"
		);
		return Ok(true);
	}
	let outgoing = self
		.services
		.sending
		.convert_to_outgoing_federation_event(pdu_json.clone())
		.await;
	debug_info!(
		room_id = %room_id,
		via = %via,
		outgoing = ?pdu_json,
		"Checking event for spam with policy server"
	);
	let response = tokio::time::timeout(
		Duration::from_secs(self.services.server.config.policy_server_request_timeout),
		self.services
			.sending
			.send_federation_request(via, PolicyRequest {
				event_id: pdu.event_id().to_owned(),
				pdu: Some(outgoing),
			}),
	)
	.await;
	let response = match response {
		| Ok(Ok(response)) => {
			debug!("Response from policy server: {:?}", response);
			response
		},
		| Ok(Err(e)) => {
			warn!(
				via = %via,
				event_id = %pdu.event_id(),
				room_id = %room_id,
				"Failed to contact policy server: {e}"
			);
			// Network or policy server errors are treated as non-fatal: event is allowed by
			// default.
			return Err(e);
		},
		| Err(elapsed) => {
			warn!(
				%via,
				event_id = %pdu.event_id(),
				%room_id,
				%elapsed,
				"Policy server request timed out after 10 seconds"
			);
			return Err!("Request to policy server timed out");
		},
	};
	trace!("Recommendation from policy server was {}", response.recommendation);
	if response.recommendation == "spam" {
		warn!(
			via = %via,
			event_id = %pdu.event_id(),
			room_id = %room_id,
			"Event was marked as spam by policy server",
		);
		return Ok(false);
	}

	Ok(true)
}
