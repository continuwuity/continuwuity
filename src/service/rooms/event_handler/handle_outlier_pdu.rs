use std::collections::{BTreeMap, HashMap, HashSet, hash_map};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_info, debug_warn, err, state_res, trace, warn,
};
use futures::{StreamExt, future::ready};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, RoomId, ServerName,
	events::StateEventType,
};

use super::{build_local_dag, check_room_id, get_room_version_rules};
use crate::rooms::timeline::pdu_fits;

impl super::Service {
	/// Handles a PDU as an outlier, performing basic checks like signatures and
	/// hashes, proclaimed event auth, and then adding it to the outlier tree.
	#[allow(clippy::too_many_arguments)]
	pub(super) async fn handle_outlier_pdu<'a, Pdu>(
		&self,
		origin: &'a ServerName,
		create_event: &'a Pdu,
		event_id: &'a EventId,
		room_id: &'a RoomId,
		mut value: CanonicalJsonObject,
		_auth_events_known: bool,
	) -> Result<(PduEvent, BTreeMap<String, CanonicalJsonValue>)>
	where
		Pdu: Event + Send + Sync,
	{
		if !pdu_fits(&mut value.clone()) {
			warn!(
				"dropping incoming PDU {event_id} in room {room_id} from {origin} because it \
				 exceeds 65535 bytes or is otherwise too large."
			);
			return Err!(Request(TooLarge("PDU is too large")));
		}
		// 1. Remove unsigned field
		value.remove("unsigned");

		// 2. Check signatures, otherwise drop
		// 3. check content hash, redact if doesn't match
		let room_version_rules = get_room_version_rules(create_event)?;
		let mut incoming_pdu = match self
			.services
			.server_keys
			.verify_event(&value, &room_version_rules)
			.await
		{
			| Ok(ruma::signatures::Verified::All) => value,
			| Ok(ruma::signatures::Verified::Signatures) => {
				// Redact
				debug_info!("Calculated hash does not match (redaction): {event_id}");
				let Ok(obj) =
					ruma::canonical_json::redact(value, &room_version_rules.redaction, None)
				else {
					return Err!(Request(InvalidParam("Redaction failed")));
				};

				// Skip the PDU if it is redacted and we already have it as an outlier event
				if self.services.timeline.pdu_exists(event_id).await {
					return Err!(Request(InvalidParam(
						"Event was redacted and we already knew about it"
					)));
				}

				obj
			},
			| Err(e) => {
				return Err!(Request(InvalidParam(debug_error!(
					"Signature verification failed for {event_id}: {e}"
				))));
			},
		};

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

		check_room_id(room_id, &pdu_event)?;

		// Fetch all auth events
		let mut auth_events: HashMap<OwnedEventId, PduEvent> = HashMap::new();

		for aid in pdu_event.auth_events() {
			if self.services.pdu_metadata.is_event_rejected(aid).await {
				debug_warn!(
					"Rejecting incoming event {} which depends on rejected auth event {aid}",
					event_id,
				);
				self.services.pdu_metadata.mark_event_rejected(event_id);
				return Err!(Request(InvalidParam("Event has rejected auth event: {aid}")));
			}

			if let Ok(auth_event) = self.services.timeline.get_pdu(aid).await {
				check_room_id(room_id, &auth_event)?;
				trace!("Found auth event {aid} for outlier event {event_id} locally");
				auth_events.insert(aid.to_owned(), auth_event);
			} else {
				debug_warn!(
					"Could not find auth event {aid} for outlier event {event_id} locally"
				);
			}
		}

		// Fetch any missing ones & reject invalid ones
		let mut missing_auth_events: HashSet<OwnedEventId> = pdu_event
			.auth_events()
			.filter(|id| !auth_events.contains_key(*id))
			.map(ToOwned::to_owned)
			.collect();

		if !missing_auth_events.is_empty() {
			debug_info!(
				"Fetching {} missing auth events for outlier event {event_id}",
				missing_auth_events.len()
			);
			let tail = self
				.services
				.state
				.get_forward_extremities(room_id)
				.collect::<Vec<_>>()
				.await;
			let backfilled = self
				.backfill_missing_events(
					room_id.to_owned(),
					HashSet::from_iter(vec![event_id.to_owned()]),
					tail,
					origin.to_owned(),
				)
				.await?;
			debug_info!("Fetched {} missing auth events for {event_id}", backfilled.len());
			let mapped = backfilled
				.iter()
				.map(|(eid, evt)| {
					let mut obj = evt.to_canonical_object();
					obj.remove("event_id"); // event_id is inserted by backfill_missing_events
					(eid.clone(), obj)
				})
				.collect::<HashMap<_, _>>();
			let local_dag = if mapped.len() == 1 {
				mapped.keys().map(ToOwned::to_owned).collect()
			} else {
				build_local_dag(&mapped).await?
			};
			debug_info!("Preparing to handle {} missing auth events", backfilled.len());
			for prev_event_id in local_dag {
				let obj = mapped
					.get(&prev_event_id)
					.expect("We should have this event in memory");
				debug_info!("Handling prev {prev_event_id}");
				let (prev, _) = Box::pin(self.handle_outlier_pdu(
					origin,
					create_event,
					&prev_event_id,
					room_id,
					obj.clone(),
					false,
				))
				.await?;
				if missing_auth_events.contains(&*prev_event_id) {
					missing_auth_events.remove(&prev_event_id);
					auth_events.insert(prev_event_id, prev);
				}
				debug_info!("Finished handling prev auth event");
			}
		} else {
			debug!("No missing auth events for outlier event {event_id}");
		}
		// reject if we are still missing some auth events.
		// If we're still missing prev events, we will fetch them individually later,
		// but there's no reason for us to be missing auth events now we've gapfilled
		// the DAG.
		if !missing_auth_events.is_empty() {
			// Don't reject: this could be a temporary condition
			return Err!(Request(InvalidParam(
				"Could not fetch all auth events for outlier event {event_id}, still missing: \
				 {missing_auth_events:?}"
			)));
		}

		// 6. Reject "due to auth events" if the event doesn't pass auth based on the
		//    auth events
		debug!("Checking based on auth events");
		let mut auth_events_by_key: HashMap<_, _> = HashMap::with_capacity(auth_events.len());
		// Build map of auth events
		for id in pdu_event.auth_events() {
			let auth_event = auth_events
				.get(id)
				.expect("we just checked that we have all auth events")
				.to_owned();

			check_room_id(room_id, &auth_event)?;

			match auth_events_by_key.entry((
				auth_event.kind.to_string().into(),
				auth_event
					.state_key
					.clone()
					.expect("all auth events have state keys"),
			)) {
				| hash_map::Entry::Vacant(v) => {
					v.insert(auth_event);
				},
				| hash_map::Entry::Occupied(_) => {
					self.services
						.outlier
						.add_pdu_outlier(pdu_event.event_id(), &incoming_pdu);
					self.services.pdu_metadata.mark_event_rejected(event_id);
					return Err!(Request(InvalidParam(
						"Auth event's type and state_key combination exists multiple times: {}, \
						 {}",
						auth_event.kind,
						auth_event.state_key().unwrap_or("")
					)));
				},
			}
		}

		// The original create event must be in the auth events
		if !matches!(
			auth_events_by_key.get(&(StateEventType::RoomCreate, String::new().into())),
			Some(_) | None
		) {
			self.services.pdu_metadata.mark_event_rejected(event_id);
			self.services
				.outlier
				.add_pdu_outlier(pdu_event.event_id(), &incoming_pdu);
			return Err!(Request(InvalidParam("Incoming event refers to wrong create event.")));
		}

		let state_fetch = |ty: &StateEventType, sk: &str| {
			let key = (ty.to_owned(), sk.into());
			ready(auth_events_by_key.get(&key).map(ToOwned::to_owned))
		};

		// PDU check: 3
		let auth_check = state_res::event_auth::auth_check(
			&room_version_rules,
			&pdu_event,
			None, // TODO: third party invite
			state_fetch,
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;

		if !auth_check {
			self.services.pdu_metadata.mark_event_rejected(event_id);
			self.services
				.outlier
				.add_pdu_outlier(pdu_event.event_id(), &incoming_pdu);
			return Err!(Request(Forbidden(
				"Event authorisation fails based on event's claimed auth events"
			)));
		}

		trace!("Validation successful.");

		// 7. Persist the event as an outlier.
		self.services
			.outlier
			.add_pdu_outlier(pdu_event.event_id(), &incoming_pdu);

		trace!("Added pdu as outlier.");

		Ok((pdu_event, incoming_pdu))
	}
}
