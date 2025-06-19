use conduwuit::{Err, Result, debug, implement, trace, warn};
use ruma::{
	EventId, OwnedEventId, OwnedServerName, RoomId, ServerName,
	api::federation::room::policy::v1::{Request as PolicyRequest, Response as PolicyResponse},
	events::{
		StateEventType,
		room::{
			policy::{PolicyServerResponseContent, RoomPolicyEventContent},
			server_acl::RoomServerAclEventContent,
		},
	},
};
use serde::{Deserialize, Serialize};

/// Returns Ok if the policy server allows the event
#[implement(super::Service)]
#[tracing::instrument(skip_all, level = "debug")]
pub async fn policyserv_check(&self, event_id: &EventId, room_id: &RoomId) -> Result {
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
	let response = self
		.services
		.sending
		.send_federation_request(via, PolicyRequest { event_id: event_id.to_owned() })
		.await;
	let response = match response {
		| Ok(response) => response,
		| Err(e) => {
			warn!("Failed to contact policy server {via} for room {room_id}: {e}");
			return Ok(());
		},
	};
	if response.recommendation == "spam" {
		warn!("Event {event_id} in room {room_id} was marked as spam by policy server {via}");
		return Err!(Request(Forbidden("Event was marked as spam by policy server")));
	};

	Ok(())
}
