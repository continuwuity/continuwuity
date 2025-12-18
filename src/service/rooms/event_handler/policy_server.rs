//! Policy server integration for event spam checking in Matrix rooms.
//!
//! This module implements a check against a room-specific policy server, as
//! described in the relevant Matrix spec proposal (see: https://github.com/matrix-org/matrix-spec-proposals/pull/4284).

use std::{collections::BTreeMap, time::Duration};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_error, debug_info, debug_warn, implement, trace,
	warn,
};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, KeyId, RoomId, ServerName, SigningKeyId,
	api::federation::room::{
		policy_check::unstable::Request as PolicyCheckRequest,
		policy_sign::unstable::Request as PolicySignRequest,
	},
	events::{StateEventType, room::policy::RoomPolicyEventContent},
};
use serde_json::value::RawValue;

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
#[tracing::instrument(skip(self, pdu, pdu_json, room_id))]
pub async fn ask_policy_server(
	&self,
	pdu: &PduEvent,
	pdu_json: &mut CanonicalJsonObject,
	room_id: &RoomId,
	incoming: bool,
) -> Result<bool> {
	if !self.services.server.config.enable_msc4284_policy_servers {
		trace!("policy server checking is disabled");
		return Ok(true); // don't ever contact policy servers
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
		.inspect_err(|e| debug_error!("failed to load room policy server state event: {e}"))
		.map(|c: RoomPolicyEventContent| c)
	else {
		debug!("room has no policy server configured");
		return Ok(true);
	};

	if self.services.server.config.policy_server_check_own_events
		&& !incoming
		&& policyserver.public_key.is_none()
	{
		// don't contact policy servers for locally generated events, but only when the
		// policy server does not require signatures
		trace!("won't contact policy server for locally generated event");
		return Ok(true);
	}

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
	if policyserver.public_key.is_some() {
		if !incoming {
			debug_info!(
				via = %via,
				outgoing = ?pdu_json,
				"Getting policy server signature on event"
			);
			return self
				.fetch_policy_server_signature(pdu, pdu_json, via, outgoing, room_id)
				.await;
		}
		// for incoming events, is it signed by <via> with the key
		// "ed25519:policy_server"?
		if let Some(CanonicalJsonValue::Object(sigs)) = pdu_json.get("signatures") {
			if let Some(CanonicalJsonValue::Object(server_sigs)) = sigs.get(via.as_str()) {
				let wanted_key_id: &KeyId<ruma::SigningKeyAlgorithm, ruma::Base64PublicKey> =
					SigningKeyId::parse("ed25519:policy_server")?;
				if let Some(CanonicalJsonValue::String(_sig_value)) =
					server_sigs.get(wanted_key_id.as_str())
				{
					// TODO: verify signature
				}
			}
		}
		debug!(
			"Event is not local and has no policy server signature, performing legacy spam check"
		);
	}
	debug_info!(
		via = %via,
		"Checking event for spam with policy server via legacy check"
	);
	let response = tokio::time::timeout(
		Duration::from_secs(self.services.server.config.policy_server_request_timeout),
		self.services
			.sending
			.send_federation_request(via, PolicyCheckRequest {
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

/// Asks a remote policy server for a signature on this event.
/// If the policy server signs this event, the original data is mutated.
#[implement(super::Service)]
#[tracing::instrument(skip_all, fields(event_id=%pdu.event_id(), via=%via))]
pub async fn fetch_policy_server_signature(
	&self,
	pdu: &PduEvent,
	pdu_json: &mut CanonicalJsonObject,
	via: &ServerName,
	outgoing: Box<RawValue>,
	room_id: &RoomId,
) -> Result<bool> {
	debug!("Requesting policy server signature");
	let response = tokio::time::timeout(
		Duration::from_secs(self.services.server.config.policy_server_request_timeout),
		self.services
			.sending
			.send_federation_request(via, PolicySignRequest { pdu: outgoing }),
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
	if response.signatures.is_none() {
		debug!("Policy server refused to sign event");
		return Ok(false);
	}
	let sigs: ruma::Signatures<ruma::OwnedServerName, ruma::ServerSigningKeyVersion> =
		response.signatures.unwrap();
	if !sigs.contains_key(via) {
		debug_warn!(
			"Policy server returned signatures, but did not include the expected server name \
			 '{}': {:?}",
			via,
			sigs
		);
		return Ok(false);
	}
	let keypairs = sigs.get(via).unwrap();
	let wanted_key_id = KeyId::parse("ed25519:policy_server")?;
	if !keypairs.contains_key(wanted_key_id) {
		debug_warn!(
			"Policy server returned signature, but did not use the key ID \
			 'ed25519:policy_server'."
		);
		return Ok(false);
	}
	let signatures_entry = pdu_json
		.entry("signatures".to_owned())
		.or_insert_with(|| CanonicalJsonValue::Object(BTreeMap::default()));

	if let CanonicalJsonValue::Object(signatures_map) = signatures_entry {
		let sig_value = keypairs.get(wanted_key_id).unwrap().to_owned();

		match signatures_map.get_mut(via.as_str()) {
			| Some(CanonicalJsonValue::Object(inner_map)) => {
				trace!("inserting PS signature: {}", sig_value);
				inner_map.insert(
					"ed25519:policy_server".to_owned(),
					CanonicalJsonValue::String(sig_value),
				);
			},
			| Some(_) => {
				debug_warn!(
					"Existing `signatures[{}]` field is not an object; cannot insert policy \
					 signature",
					via
				);
				return Ok(false);
			},
			| None => {
				let mut inner = BTreeMap::new();
				inner.insert(
					"ed25519:policy_server".to_owned(),
					CanonicalJsonValue::String(sig_value.clone()),
				);
				trace!(
					"created new signatures object for {via} with the signature {}",
					sig_value
				);
				signatures_map.insert(via.as_str().to_owned(), CanonicalJsonValue::Object(inner));
			},
		}
	} else {
		debug_warn!(
			"Existing `signatures` field is not an object; cannot insert policy signature"
		);
		return Ok(false);
	}
	Ok(true)
}
