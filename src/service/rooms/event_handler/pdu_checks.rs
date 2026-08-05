use std::collections::HashMap;

use base64::Engine;
use conduwuit::{
	Err, Event, EventTypeExt, PduEvent, Result, debug, debug::DebugInspect, debug_error,
	debug_info, err, info, matrix::StateKey, state_res, trace,
};
use futures::future::ready;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, ServerName,
	api::error::ErrorKind,
	canonical_json::redact,
	events::StateEventType,
	room_version_rules::{EventIdFormatVersion, RoomVersionRules},
};

use crate::rooms::{
	event_handler::parse_incoming_pdu::expect_event_id_array, timeline::pdu_fits,
};

/// Checks that the given event ID matches the expected format by attempting to
/// decode the base64 string.
fn check_event_id_format(
	event_id: &EventId,
	room_version_rules: &RoomVersionRules,
) -> Result<Vec<u8>> {
	let event_id_without_sigil = event_id
		.as_str()
		.strip_prefix("$")
		.expect("event ID must start with a $ sigil");
	let b64_alphabet = match room_version_rules.event_id_format {
		| EventIdFormatVersion::V2 => base64::alphabet::STANDARD,
		| EventIdFormatVersion::V3 => base64::alphabet::URL_SAFE,
		| _ => return Err!("Unsupported event ID Format"),
	};
	let b64_engine = base64::engine::GeneralPurpose::new(
		&b64_alphabet,
		base64::engine::general_purpose::NO_PAD,
	);
	b64_engine.decode(event_id_without_sigil).map_err(|e| {
		err!(Request(InvalidParam(debug_error!(
			error=?e,
			"PDU references an invalid event ID: {event_id}"
		))))
	})
}

impl super::Service {
	/// Checks that the PDU conforms to the PDU format (check 1). This is
	/// already mostly done during deserialisation, so this function just checks
	/// that the PDU isn't a too large.
	pub fn pdu_format_check_1(
		pdu_json: &CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
		create_event_id: &EventId,
	) -> Result<()> {
		let event_format = &room_version_rules.event_format;
		// NOTE: if we do any more validation outside of deserialisation, it has to be
		// done here.

		if !pdu_fits(pdu_json) {
			return Err!(Request(TooLarge("PDU is too large")));
		}

		if event_format.require_room_create_room_id {
			if pdu_json.get("room_id").is_none() {
				return Err!(Request(BadJson("Missing required PDU field: `room_id`")));
			}
		}

		let auth_events = expect_event_id_array(pdu_json, "auth_events")?;
		if auth_events.len() > 10 {
			return Err!(Request(BadJson("PDU has too many auth events")));
		}
		for auth_event_id in &auth_events {
			check_event_id_format(auth_event_id, room_version_rules)?;
		}

		// The m.room.create event is the genesis event and has empty auth_events
		// by definition, so it is exempt from the checks below requiring or
		// forbidding the create event in auth_events (it cannot reference itself).
		let Some(event_type) = pdu_json.get("type").and_then(CanonicalJsonValue::as_str) else {
			return Err!(Request(BadJson("PDU is missing a type")));
		};
		let state_key = pdu_json
			.get("state_key")
			.and_then(CanonicalJsonValue::as_str);

		let is_create_event = event_type == "m.room.create" && state_key == Some("");

		if !is_create_event {
			let create_event_in_auth_events = auth_events.iter().any(|id| id == create_event_id);
			if !event_format.allow_room_create_in_auth_events && create_event_in_auth_events {
				return Err!(Request(BadJson("PDU references a create event")));
			} else if event_format.allow_room_create_in_auth_events
				&& !create_event_in_auth_events
			{
				return Err!(Request(BadJson("PDU does not reference the room create event")));
			}
		}

		let prev_events = expect_event_id_array(pdu_json, "prev_events")?;
		if prev_events.len() > 20 {
			return Err!(Request(BadJson("PDU has too many prev events")));
		}
		for prev_event_id in &prev_events {
			check_event_id_format(prev_event_id, room_version_rules)?;
		}

		Ok(())
	}

	/// Checks that the PDU has a valid signature (check 2), and redacts it if
	/// the content hash verification fails (check 3), returning the
	/// potentially modified JSON. Returns an error if the PDU cannot be
	/// redacted, or fails signature verification.
	pub async fn signature_hash_check_2_3(
		&self,
		pdu_json: CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result<CanonicalJsonObject> {
		match self
			.services
			.server_keys
			.verify_event(&pdu_json, room_version_rules)
			.await
		{
			| Ok(ruma::signatures::Verified::All) => {
				trace!("Signatures and hashes verified successfully");
				Ok(pdu_json)
			},
			| Ok(ruma::signatures::Verified::Signatures) => {
				debug_info!("Content hash mismatch, redacting event and continuing");
				let redacted = redact(pdu_json, &room_version_rules.redaction, None)
					.map_err(|e| err!(Request(BadJson("Unable to redact event: {e}"))))?;
				Ok(redacted)
			},
			| Err(e) => {
				Err!(Request(Forbidden(debug_error!("Signature verification failed: {e}"))))
			},
		}
	}

	/// Checks PDU check 4: Passes authorisation rules based on the event's auth
	/// events ([spec]).
	///
	/// If the auth check fails, false is returned, otherwise true.
	///
	/// [spec]: https://spec.matrix.org/v1.19/server-server-api/#checks-performed-on-receipt-of-a-pdu
	pub async fn auth_state_check_4(
		&self,
		pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		create_event: &PduEvent,
		auth_events_by_key: &HashMap<(StateEventType, StateKey), PduEvent>,
	) -> Result<bool> {
		let state_fetch = |ty: &StateEventType, sk: &str| {
			let key = (ty.to_owned(), sk.into());
			ready(auth_events_by_key.get(&key).map(ToOwned::to_owned))
		};

		state_res::event_auth::auth_check(
			room_version_rules,
			pdu,
			None, // TODO: third party invite
			state_fetch,
			create_event,
		)
		.await
		.map_err(|e| err!("Event self-authentication failed: {e:?}"))
	}

	/// Checks that the event passes PDU check 5, which ensures that the event
	/// is authorised based on the state before the event (which is the resolved
	/// state across all prev events).
	///
	/// Returns a boolean indicating whether the event is authorised, and also
	/// the resolved state before the event for later use. Returns an error if
	/// state fetching or auth checking fails.
	pub(super) async fn state_before_check_5(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		create_event: &PduEvent,
		origin: &ServerName,
	) -> Result<(bool, HashMap<u64, OwnedEventId>)> {
		debug!(
			event_id = %incoming_pdu.event_id,
			"Resolving state at event"
		);
		let room_id = incoming_pdu.room_id_or_hash();

		// If the incoming event only has one prev event, we can just use the state at
		// that event, but otherwise we have to resolve across each fork. If we're
		// missing even one of the prev events, we have to ask a remote server for help.
		//
		// TODO: this can be optimised by only loading auth chain events into memory,
		// rather than the entire state.
		let state_before = self
			.state_before_incoming(&incoming_pdu, room_version_rules)
			.await?;
		let state_before = match state_before {
			| Some(s) => s,
			| None => {
				trace!("Could not calculate incoming state, asking remote {origin} for it");
				self.fetch_state(origin, create_event, &room_id, incoming_pdu.event_id())
					.await
					.inspect_err(|e| {
						debug_error!("Could not fetch state from {origin}: {e}");
					})?
			},
		};

		if state_before.is_empty()
			&& *incoming_pdu.event_type() != StateEventType::RoomCreate.into()
		{
			// This can happen if the remote sends an event but cannot be reached to fetch
			// the state at it, and all other servers in the room (which might just be the
			// unreachable server) are unable to provide required info.
			// returning an error here allows the upgrade to be attempted at another time.
			return Err!(Request(Forbidden("Could not resolve incoming state before event")));
		}
		trace!(state_events = state_before.len(), "Calculated incoming state");

		let state_fetch_state = &state_before;
		let state_fetch = |k: StateEventType, s: StateKey| async move {
			let shortstatekey = self.services.short.get_shortstatekey(&k, &s).await.ok()?;

			let event_id = state_fetch_state.get(&shortstatekey)?;
			self.services.timeline.get_pdu(event_id).await.ok()
		};

		debug!(
			event_id = %incoming_pdu.event_id,
			"Running state-before auth check"
		);

		// PDU check: 5
		let auth_check = state_res::event_auth::auth_check(
			room_version_rules,
			incoming_pdu,
			None, // TODO: third party invite
			|ty, sk| state_fetch(ty.clone(), sk.into()),
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;
		Ok((auth_check, state_before))
	}

	/// Checks that the event passes PDU check 6, which ensures that the event
	/// is authorised based on the room's current state (which is the resolved
	/// state across all current forward extremities).
	///
	/// Returns a boolean indicating whether the event is authorised, or an
	/// error if the auth check fails.
	pub(super) async fn current_state_check_6(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		create_event: &PduEvent,
	) -> Result<bool> {
		debug!(
			event_id = %incoming_pdu.event_id,
			"Gathering auth events"
		);
		let auth_events = self
			.services
			.state
			.get_auth_events(
				&incoming_pdu.room_id_or_hash(),
				incoming_pdu.kind(),
				incoming_pdu.sender(),
				incoming_pdu.state_key(),
				incoming_pdu.content(),
				room_version_rules,
			)
			.await?;

		let state_fetch = |k: &StateEventType, s: &str| {
			let key = k.with_state_key(s);
			ready(auth_events.get(&key).map(ToOwned::to_owned))
		};

		debug!(
			event_id = %incoming_pdu.event_id,
			"Running current state auth check"
		);
		state_res::event_auth::auth_check(
			room_version_rules,
			incoming_pdu,
			None, // third-party invite
			state_fetch,
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))
	}

	/// Performs PDU check 7 - does the policy server allow this event.
	///
	/// If the policy server forbids the event, false is returned. If there is a
	/// problem contacting the policy server, or it returns an unrecognised
	/// response, an appropriate error is returned.
	pub(super) async fn policy_server_check_7(
		&self,
		incoming_pdu: &PduEvent,
		pdu_json: &mut CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result<bool> {
		let event_id = pdu_json
			.remove("event_id")
			.expect("event_id should be present in pdu_json at this stage");
		if let Err(e) = self
			.policy_server_allows_event(
				incoming_pdu,
				pdu_json,
				&incoming_pdu.room_id_or_hash(),
				room_version_rules,
				true,
			)
			.await
			.debug_inspect(|()| {
				debug!(
					event_id = %incoming_pdu.event_id,
					"Event has passed policy server check."
				);
			}) {
			return if matches!(e.kind(), ErrorKind::Forbidden) {
				info!(
					event_id = %incoming_pdu.event_id,
					error = %e,
					"Event has been marked as spam by policy server: {}",
					e.message(),
				);
				Ok(false)
			} else {
				Err(e)
			};
		}
		pdu_json.insert("event_id".to_owned(), event_id);
		Ok(true)
	}
}

#[cfg(test)]
mod tests {
	use ruma::server_name;

	use super::*;

	#[test]
	fn v1_event_id_always_errors() {
		let v1_event_id = EventId::new_v1(server_name!("example.com"));
		assert!(
			check_event_id_format(&v1_event_id, &RoomVersionRules::V3).is_err(),
			"V1 event ID should not be valid in room V3"
		);
		assert!(
			check_event_id_format(&v1_event_id, &RoomVersionRules::V4).is_err(),
			"V1 event ID should not be valid in room V4"
		);
	}

	#[test]
	fn v2_event_id_ok_in_room_v3_only() {
		let v2_event_id = EventId::new_v2_or_v3("KtY/RFXNXYxprwSOypvTlZsbohReRw19qcPATZDda4E")
			.expect("fixture event hash must be valid");
		let decoded_bytes = check_event_id_format(&v2_event_id, &RoomVersionRules::V3)
			.expect("V2 event ID should be valid in room V3");
		assert_eq!(
			decoded_bytes,
			vec![
				42, 214, 63, 68, 85, 205, 93, 140, 105, 175, 4, 142, 202, 155, 211, 149, 155, 27,
				162, 20, 94, 71, 13, 125, 169, 195, 192, 77, 144, 221, 107, 129
			],
			"V2 event reference hash did not decode to expected bytes"
		);
		assert!(
			check_event_id_format(&v2_event_id, &RoomVersionRules::V4).is_err(),
			"V2 event ID should not be valid in room V4"
		);
	}

	#[test]
	fn v3_event_id_errors_in_room_v3() {
		let v3_event_id = EventId::new_v2_or_v3("zsj67_pqjr5qqh5GMTXqxLM0FqjP5OLrvXO0PjwWe88")
			.expect("fixture event hash must be valid");
		// Since the urlsafe replacements aren't in the standard base64 alphabet, this
		// simply errors instead of decoding to potentially incorrect bytes.
		assert!(
			check_event_id_format(&v3_event_id, &RoomVersionRules::V3).is_err(),
			"V3 event ID should not be valid in room V3"
		);
	}
	#[test]
	fn v3_event_id_ok_in_room_v4_onward() {
		let v3_event_id = EventId::new_v2_or_v3("zsj67_pqjr5qqh5GMTXqxLM0FqjP5OLrvXO0PjwWe88")
			.expect("fixture event hash must be valid");
		// Since the urlsafe replacements aren't in the standard base64 alphabet, this
		// simply errors instead of decoding to potentially incorrect bytes.
		let expected_bytes = vec![
			206, 200, 250, 239, 250, 106, 142, 190, 106, 170, 30, 70, 49, 53, 234, 196, 179, 52,
			22, 168, 207, 228, 226, 235, 189, 115, 180, 62, 60, 22, 123, 207,
		];
		assert_eq!(
			check_event_id_format(&v3_event_id, &RoomVersionRules::V4)
				.expect("V3 event should be valid room V4"),
			expected_bytes,
			"V3 event ID in room V4 did not decode to expected bytes"
		);
		// These versions didn't change the algorithm, but might as well test them
		// anyway
		assert_eq!(
			check_event_id_format(&v3_event_id, &RoomVersionRules::V6)
				.expect("V3 event should be valid room V6"),
			expected_bytes,
			"V3 event ID in room V6 did not decode to expected bytes"
		);
		assert_eq!(
			check_event_id_format(&v3_event_id, &RoomVersionRules::V10)
				.expect("V3 event should be valid room V10"),
			expected_bytes,
			"V3 event ID in room V10 did not decode to expected bytes"
		);
		assert_eq!(
			check_event_id_format(&v3_event_id, &RoomVersionRules::V12)
				.expect("V3 event should be valid room V12"),
			expected_bytes,
			"V3 event ID in room V12 did not decode to expected bytes"
		);
	}
}
