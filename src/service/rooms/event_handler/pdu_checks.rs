use std::collections::HashMap;

use conduwuit::{
	Err, Event, EventTypeExt, PduEvent, Result, debug, debug::DebugInspect, debug_error,
	debug_info, err, info, matrix::StateKey, state_res, trace,
};
use futures::future::ready;
use ruma::{
	CanonicalJsonObject, OwnedEventId, ServerName, api::error::ErrorKind, canonical_json::redact,
	events::StateEventType, room_version_rules::RoomVersionRules,
};

use crate::rooms::timeline::pdu_fits;

impl super::Service {
	/// Checks that the PDU conforms to the PDU format (check 1). This is
	/// already mostly done during deserialisation, so this function just checks
	/// that the PDU isn't a too large.
	pub(super) fn pdu_format_check_1(pdu_json: &CanonicalJsonObject) -> bool {
		// NOTE: if we do any more validation outside of deserialisation, it has to be
		// done here.
		pdu_fits(pdu_json)
	}

	/// Checks that the PDU has a valid signature (check 2), and redacts it if
	/// the content hash verification fails (check 3), returning the
	/// potentially modified JSON. Returns an error if the PDU cannot be
	/// redacted, or fails signature verification.
	pub(super) async fn signature_hash_check_2_3(
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
			| Ok(ruma::signatures::Verified::All) => Ok(pdu_json),
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
	pub(super) async fn auth_state_check_4(
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
					.debug_inspect_err(|e| {
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
