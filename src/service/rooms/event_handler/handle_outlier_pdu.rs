use std::collections::{BTreeMap, HashMap, hash_map};

use conduwuit::{
	Err, Event, EventTypeExt, PduEvent, Result, debug, debug_warn, err, info, trace,
};
use ruma::{CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, RoomId, ServerName};

use super::get_room_version_rules;

impl super::Service {
	/// Handles a PDU as an outlier, performing basic checks like signatures and
	/// hashes, proclaimed event auth, and then adding it to the outlier tree.
	///
	/// This performs steps 1 through 4 of [S-S section 5.1][spec], returning
	/// the parsed PDU and modified JSON object.
	///
	/// [spec]: https://spec.matrix.org/v1.19/server-server-api/#checks-performed-on-receipt-of-a-pdu
	#[allow(clippy::too_many_arguments)]
	#[tracing::instrument(name = "handle_outlier", skip_all)]
	pub(super) async fn handle_outlier_pdu<'a, Pdu>(
		&self,
		origin: &'a ServerName,
		create_event: &'a Pdu,
		event_id: &'a EventId,
		room_id: &'a RoomId,
		mut value: CanonicalJsonObject,
	) -> Result<(PduEvent, BTreeMap<String, CanonicalJsonValue>)>
	where
		Pdu: Event + Send + Sync,
	{
		// Skip outlier handling if we already have this event as either a timeline or
		// outlier PDU.
		if let Ok(pdu_event) = self.services.timeline.get_pdu(event_id).await {
			debug!(
				"Database hit for {event_id} (event is either an outlier or already promoted), \
				 skipping outlier handling"
			);
			value.insert(
				"event_id".to_owned(),
				CanonicalJsonValue::String(event_id.as_str().to_owned()),
			);
			return Ok((pdu_event, value));
		}

		// 1. Check that the PDU follows the format for the room version
		// (in this case, just size check)
		let room_version_rules = get_room_version_rules(create_event)?;
		Self::pdu_format_check_1(&value, &room_version_rules, create_event.event_id())
			.inspect_err(|e| {
				info!(
					err=?e,
					"Dropping incoming PDU from {origin} because it violates the room event format"
				);
			})?;

		value.remove("unsigned");
		let room_version_rules = get_room_version_rules(create_event)?;

		// 2. Check signatures, otherwise drop.
		// 3. Check content hash, redacting the event if it fails.
		let mut incoming_pdu = self
			.signature_hash_check_2_3(value, &room_version_rules)
			.await?;

		// Now that we have checked the signature and hashes we can add the eventID and
		// convert to our PduEvent type
		incoming_pdu.insert(
			"event_id".to_owned(),
			CanonicalJsonValue::String(event_id.as_str().to_owned()),
		);
		let pdu_event = serde_json::from_value::<PduEvent>(
			serde_json::to_value(&incoming_pdu).expect("CanonicalJsonObj is a valid JsonValue"),
		)
		.map_err(|e| err!(Request(BadJson(debug_warn!("Event is not a valid PDU: {e}")))))?;

		// TODO(nex): From hereon the event is not dropped, and thus always added as an
		// outlier. However, we only do that at the end of this function, which means
		// several duplicated calls to add_pdu_outlier. Shouldn't we just do it here
		// instead, since we know it's going to be persisted as an outlier no matter
		// what? the rest of this function is basically just to check PDU check 4.

		// NOTE^: Technically, persisting the event before knowing if it's rejected
		// introduces a race condition in fetch_and_persist_event_auth, where we have
		// the event locally, but haven't yet flagged it as rejected, which the
		// fetcher perceives as "accepted". I'm not sure if that's practically possible
		// though.

		// Fetch all auth events
		let mut auth_events: HashMap<OwnedEventId, PduEvent> = HashMap::new();

		for auth_event_id in pdu_event.auth_events() {
			if let Ok(auth_event) = self.services.timeline.get_pdu(auth_event_id).await {
				trace!("Found auth event {auth_event_id} for outlier event {event_id} locally");
				auth_events.insert(auth_event_id.to_owned(), auth_event);
			} else {
				debug_warn!(
					"Could not find auth event {auth_event_id} for outlier event {event_id} \
					 locally"
				);
			}
		}

		// Fetch any missing ones & reject invalid ones
		if auth_events.len() != pdu_event.auth_events().count() {
			info!("Missing some auth events, asking remote for auth chain");
			let auth_chain_map = self
				.fetch_and_persist_event_auth(
					&pdu_event,
					origin,
					&room_version_rules,
					create_event,
				)
				.await?;
			for auth_event_id in pdu_event.auth_events() {
				if auth_events.contains_key(auth_event_id) {
					continue;
				}
				if let Some(auth_event) = auth_chain_map.get(auth_event_id) {
					auth_events.insert(auth_event_id.to_owned(), auth_event.clone());
				} else {
					return Err!(Request(Forbidden(
						"Remote server is not divulging incoming event's auth events (missing: \
						 {auth_event_id})"
					)));
				}
			}
		}

		// Ensure none of the auth events are rejected - if they are, reject too.
		for (auth_event_id, auth_event) in &auth_events {
			if self
				.services
				.pdu_metadata
				.is_event_rejected(auth_event_id)
				.await
			{
				debug_warn!(
					"Rejecting incoming event {} which depends on rejected auth event \
					 {auth_event_id}",
					event_id,
				);
				self.reject_and_persist(event_id, &incoming_pdu);
				return Err!(Request(Forbidden(
					"Event has rejected auth event: {auth_event_id}"
				)));
			}
			if auth_event.room_id_or_hash() != room_id {
				debug_warn!(
					%auth_event_id,
					auth_event_room_id=%auth_event.room_id_or_hash(),
					expected_room_id=%room_id,
					"Rejecting incoming event which depends on an auth event in another room.",
				);
				self.reject_and_persist(event_id, &incoming_pdu);
				return Err!(Request(Forbidden(
					"Event depends on a cross-room auth event: {auth_event_id}"
				)));
			}
		}

		// 4. Reject "due to auth events" if the event doesn't pass auth based on the
		//    claimed auth events
		debug!("Checking based on auth events");
		let mut auth_events_by_key: HashMap<_, _> = HashMap::with_capacity(auth_events.len());
		// Build map of auth events
		for id in pdu_event.auth_events() {
			let auth_event = auth_events
				.get(id)
				.expect("we just checked that we have all auth events")
				.to_owned();

			let key = auth_event.kind().with_state_key(
				auth_event
					.state_key
					.clone()
					.expect("all auth events must have state keys"),
			);
			match auth_events_by_key.entry(key) {
				| hash_map::Entry::Vacant(v) => {
					v.insert(auth_event);
				},
				| hash_map::Entry::Occupied(_) => {
					self.reject_and_persist(event_id, &incoming_pdu);
					return Err!(Request(Forbidden(debug_warn!(
						"Auth event's type and state_key combination exists multiple times: {}, \
						 {}",
						auth_event.kind,
						auth_event.state_key().unwrap_or("")
					))));
				},
			}
		}

		if !self
			.auth_state_check_4(
				&pdu_event,
				&room_version_rules,
				create_event.as_pdu(),
				&auth_events_by_key,
			)
			.await?
		{
			self.reject_and_persist(event_id, &incoming_pdu);
			return Err!(Request(Forbidden(debug_warn!(
				"Event authorisation fails based on event's claimed auth events"
			))));
		}

		// 7. Persist the event as an outlier.
		self.services
			.outlier
			.add_pdu_outlier(pdu_event.event_id(), &incoming_pdu);

		debug!("PDU passed checks and has been persisted as an outlier");

		Ok((pdu_event, incoming_pdu))
	}

	/// Marks the event as rejected and then saves it as an outlier.
	pub(super) fn reject_and_persist(&self, event_id: &EventId, pdu: &CanonicalJsonObject) {
		self.services.pdu_metadata.mark_event_rejected(event_id);
		self.services.outlier.add_pdu_outlier(event_id, pdu);
	}
}
