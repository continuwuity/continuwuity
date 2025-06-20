use conduwuit::{
	Err, Event, PduEvent, Result, debug, implement, utils::to_canonical_object, warn,
};
use ruma::{
	RoomId, ServerName,
	api::federation::room::policy::v1::Request as PolicyRequest,
	canonical_json::to_canonical_value,
	events::{StateEventType, room::policy::RoomPolicyEventContent},
};

/// Returns Ok if the policy server allows the event
#[implement(super::Service)]
#[tracing::instrument(skip_all, level = "debug")]
pub async fn policyserv_check(&self, pdu: &PduEvent, room_id: &RoomId) -> Result {
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
	// TODO: dont do *this*
	let pdu_json = self.services.timeline.get_pdu_json(pdu.event_id()).await?;
	let outgoing = self
		.services
		.sending
		.convert_to_outgoing_federation_event(pdu_json)
		.await;
	// let s = match serde_json::to_string(outgoing.as_ref()) {
	// 	| Ok(s) => s,
	// 	| Err(e) => {
	// 		warn!("Failed to convert pdu {} to outgoing federation event: {e}",
	// pdu.event_id()); 		return Err!(Request(InvalidParam("Failed to convert PDU
	// to outgoing event."))); 	},
	// };
	debug!("Checking pdu {outgoing:?} for spam with policy server {via} for room {room_id}");
	let response = self
		.services
		.sending
		.send_federation_request(via, PolicyRequest {
			event_id: pdu.event_id().to_owned(),
			pdu: Some(outgoing),
		})
		.await;
	let response = match response {
		| Ok(response) => response,
		| Err(e) => {
			warn!("Failed to contact policy server {via} for room {room_id}: {e}");
			return Ok(());
		},
	};
	if response.recommendation == "spam" {
		warn!(
			"Event {} in room {room_id} was marked as spam by policy server {via}",
			pdu.event_id().to_owned()
		);
		return Err!(Request(Forbidden("Event was marked as spam by policy server")));
	};

	Ok(())
}
