//! Policy server integration for event spam checking in Matrix rooms.
//!
//! This module implements a check against a room-specific policy server, as
//! described in the relevant Matrix spec proposal (see: https://github.com/matrix-org/matrix-spec-proposals/pull/4284).

use std::{collections::BTreeMap, time::Duration};

use conduwuit::{
	Err, Error, Event, PduEvent, Result, debug, debug_info, debug_warn, err, error, implement,
	info, trace, warn,
};
use http::StatusCode;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, KeyId, RoomId, RoomVersionId, ServerName,
	api::{
		client::error::ErrorKind, federation::room::policy_sign::v1::Request as PolicySignRequest,
	},
	canonical_json::redact,
	events::{StateEventType, room::policy::UnstableRoomPolicyEventContent},
	serde::{Base64, base64::Standard},
	signatures::{Ed25519Verifier, canonical_json},
};
use serde_json::value::RawValue;
use tokio::{join, time::sleep};

pub(super) fn verify_policy_signature(
	via: &ServerName,
	ps_key: &Base64<Standard, Vec<u8>>,
	pdu_json: &CanonicalJsonObject,
	room_version: &RoomVersionId,
) -> bool {
	let Some(canonical_json) = redact(pdu_json.clone(), room_version, None)
		.ok()
		.and_then(|r| canonical_json(r).ok())
	else {
		return false;
	};
	let Some(CanonicalJsonValue::Object(signature_map)) = pdu_json.get("signatures") else {
		return false;
	};
	let Some(CanonicalJsonValue::Object(signature_set)) = signature_map.get(via.as_str()) else {
		return false;
	};
	let Some(signature) = signature_set
		.get("ed25519:policy_server")
		.and_then(|s| s.as_str())
		.and_then(|s| Base64::parse(s).ok())
	else {
		return false;
	};

	trace!(%signature, "Verifying policy server signature");
	ruma::signatures::verify_json_with(&Ed25519Verifier, ps_key, &signature, &canonical_json)
		.inspect_err(|error| debug_warn!(%error, "Policy server signature verification failed"))
		.is_ok()
}

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
#[tracing::instrument(skip(self, pdu, pdu_json), level = "info")]
pub async fn policy_server_allows_event(
	&self,
	pdu: &PduEvent,
	pdu_json: &mut CanonicalJsonObject,
	room_id: &RoomId,
	room_version: &RoomVersionId,
	incoming: bool,
) -> Result<()> {
	let ps = match StateEventType::from(pdu.event_type().clone()) {
		| StateEventType::RoomPolicy | StateEventType::UnstableRoomPolicy => return Ok(()),
		| _ => {
			let (stable, unstable) = join!(
				self.services
					.state_accessor
					.room_state_get_content::<UnstableRoomPolicyEventContent>(
						room_id,
						&StateEventType::RoomPolicy,
						"",
					),
				self.services
					.state_accessor
					.room_state_get_content::<UnstableRoomPolicyEventContent>(
						room_id,
						&StateEventType::UnstableRoomPolicy,
						"",
					)
			);
			if stable.is_ok() { stable } else { unstable }
		},
	};
	let ps = match ps {
		| Ok(ps) => ps,
		| Err(e) => {
			if e.is_not_found() {
				trace!("no policy server configured");
				return Ok(()); // no policy server configured
			}
			err!("failed to load policy server event");
			return Err(e);
		},
	};

	let ps_key = match ps.effective_key() {
		| Ok(key) => key,
		| Err(e) => {
			debug!(
				error=%e,
				"room has a policy server configured, but no valid public keys; skipping spam check"
			);
			return Ok(());
		},
	};

	let Some(via) = ps.via.as_ref().and_then(|via| ServerName::parse(via).ok()) else {
		trace!("No via configured for room policy server, skipping spam check");
		return Ok(());
	};

	if via.is_empty() {
		trace!("Policy server is empty for room {room_id}, skipping spam check");
		return Ok(());
	}
	if via == self.services.globals.server_name()
		&& !self.services.server.config.federation_loopback
	{
		warn!(
			%via,
			%room_id,
			"Cannot ask ourselves for a policy signature if `federation_loopback=false`",
		);
		return Ok(());
	}

	if !self.services.state_cache.server_in_room(via, room_id).await {
		debug!(
			via = %via,
			"Policy server is not in the room, skipping spam check"
		);
		return Ok(());
	}

	if incoming {
		// Verify the signature instead of calling a check
		if verify_policy_signature(via, &ps_key, pdu_json, room_version) {
			debug!(
				via = %via,
				"Event is incoming and has a valid policy server signature"
			);
			return Ok(());
		}
		debug_info!(
			via = %via,
			"Event is incoming but does not have a valid policy server signature; asking policy \
			server to sign it now"
		);
	}

	let outgoing = self
		.services
		.sending
		.convert_to_outgoing_federation_event(pdu_json.clone())
		.await;

	debug_info!(
		via = %via,
		"Asking policy server to sign event"
	);
	self.fetch_policy_server_signature(pdu, pdu_json, via, outgoing, room_id, ps_key, 0)
		.await
}
#[allow(clippy::too_many_arguments)]
#[implement(super::Service)]
async fn handle_policy_server_error(
	&self,
	error: Error,
	pdu: &PduEvent,
	pdu_json: &mut CanonicalJsonObject,
	via: &ServerName,
	outgoing: Box<RawValue>,
	room_id: &RoomId,
	policy_server_key: Base64<Standard, Vec<u8>>,
	retries: u8,
	timeout: Duration,
) -> Result<()> {
	match error.status_code() {
		| StatusCode::OK => unreachable!("ok response passed to handle_policy_server_error"),
		| StatusCode::BAD_REQUEST => {
			if matches!(error.kind(), ErrorKind::Forbidden { .. }) {
				warn!(
					via = %via,
					event_id = %pdu.event_id(),
					room_id = %room_id,
					error = ?error,
					"Policy server marked the event as spam"
				);
				return Err(error);
			}
			error!(
				via = %via,
				event_id = %pdu.event_id(),
				room_id = %room_id,
				error = ?error,
				"Policy server could not understand our request: {}",
				error.kind(),
			);
			Err!(BadServerResponse("Error communicating with policy server"))
		},
		| StatusCode::FORBIDDEN => {
			Err!(Request(Forbidden(
				"Policy server refused to sign the event due to the room ACL"
			)))
		},
		| StatusCode::NOT_FOUND => {
			debug_info!(
				via = %via,
				event_id = %pdu.event_id(),
				room_id = %room_id,
				"Policy server is not actually a policy server or is not protecting this room: {}",
				error.message()
			);
			Ok(())
		},
		| StatusCode::TOO_MANY_REQUESTS => {
			if let Some(retry_after) = error.retry_after() {
				if retries >= 5 {
					warn!(
						via = %via,
						event_id = %pdu.event_id(),
						room_id = %room_id,
						retries,
						"Policy server rate-limited us too many times; giving up"
					);
					return Err(error); // Error should be passed to c2s
				}
				let saturated = retry_after.min(timeout);
				// ^ don't wait more than 60 seconds
				info!(
					via = %via,
					event_id = %pdu.event_id(),
					room_id = %room_id,
					retry_after = %saturated.as_secs(),
					retries,
					"Policy server rate-limited us; retrying after {retry_after:?}"
				);
				// TODO: select between this sleep and shutdown signal
				sleep(saturated).await;
				if !self.services.server.running() {
					return Err(error);
				}
				return Box::pin(self.fetch_policy_server_signature(
					pdu,
					pdu_json,
					via,
					outgoing,
					room_id,
					policy_server_key,
					retries.saturating_add(1),
				))
				.await;
			}
			warn!(
				via = %via,
				event_id = %pdu.event_id(),
				room_id = %room_id,
				retries,
				"Policy server rate-limited us without giving a retry window; giving up"
			);
			Err(error)
		},
		| _ => Err!(BadServerResponse(
			"Unexpected response from policy server: {}/{}",
			error.status_code(),
			error.kind().to_string()
		)),
	}
}

/// Asks a remote policy server for a signature on this event.
/// If the policy server signs this event, the original data is mutated.
#[allow(clippy::too_many_arguments)]
#[implement(super::Service)]
#[tracing::instrument(skip_all, fields(event_id=%pdu.event_id(), via=%via), level = "info")]
pub async fn fetch_policy_server_signature(
	&self,
	pdu: &PduEvent,
	pdu_json: &mut CanonicalJsonObject,
	via: &ServerName,
	outgoing: Box<RawValue>,
	room_id: &RoomId,
	policy_server_key: Base64<Standard, Vec<u8>>,
	retries: u8,
) -> Result<()> {
	let timeout = Duration::from_secs(self.services.server.config.policy_server_request_timeout);
	debug!("Requesting policy server signature");
	let response = tokio::time::timeout(
		timeout,
		self.services
			.sending
			.send_federation_request(via, PolicySignRequest { pdu: outgoing.clone() }),
	)
	.await;

	let response = match response {
		| Ok(Ok(response)) => {
			debug!("Response from policy server: {:?}", response);
			response
		},
		| Ok(Err(e)) => {
			return self
				.handle_policy_server_error(
					e,
					pdu,
					pdu_json,
					via,
					outgoing,
					room_id,
					policy_server_key,
					retries,
					timeout,
				)
				.await;
		},
		| Err(elapsed) => {
			warn!(
				%via,
				event_id = %pdu.event_id(),
				%room_id,
				%elapsed,
				"Policy server signature request timed out"
			);
			return Err!(Request(Forbidden("Policy server did not respond in time")));
		},
	};

	if response
		.signatures
		.as_ref()
		.is_none_or(|sigs| !sigs.contains_key(via))
	{
		error!(
			%via,
			"Policy server did not sign event: {:?}",
			response.signatures
		);
		return Err!(BadServerResponse(
			"Policy server did not include expected server name in signatures"
		));
	}
	// Unwraps are safe here because we checked both in the above if statement
	let signatures = response.signatures.unwrap();
	let keypairs = signatures.get(via).unwrap();

	// TODO: need to be able to verify other algorithms
	let wanted_key_id = KeyId::parse("ed25519:policy_server")?;
	if !keypairs.contains_key(wanted_key_id) {
		error!(
			signatures = ?signatures,
			"Policy server returned signatures, but did not use the key ID \
			 'ed25519:policy_server'."
		);
		return Err!(BadServerResponse(
			"Policy server signed the event, but did not use the expected key ID"
		));
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
				// This should never happen
				unreachable!(
					"Existing `signatures[{}]` field is not an object; cannot insert policy \
					 signature",
					via
				);
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
		// TODO: verify signature value was made with the policy_server_key
		// rather than the expected key.
	} else {
		unreachable!(
			"Existing `signatures` field is not an object; cannot insert policy signature"
		);
	}
	debug_info!("Policy server allowed event");
	Ok(())
}
