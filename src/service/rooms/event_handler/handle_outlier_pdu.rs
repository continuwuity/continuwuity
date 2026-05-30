use std::collections::{BTreeMap, HashMap, hash_map};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_info, debug_warn, err, implement, info, state_res,
	trace, warn,
};
use futures::future::ready;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, RoomId, ServerName,
	api::federation::authorization::get_event_authorization, canonical_json::redact,
	events::StateEventType,
};

use super::{check_room_id, get_room_version_rules};
use crate::rooms::timeline::pdu_fits;

#[implement(super::Service)]
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
		| Ok(ruma::signatures::Verified::All) => {
			if let Ok(pdu_event) = self.services.timeline.get_pdu(event_id).await {
				debug!(
					"Already have event {event_id} as an outlier or timeline event, not \
					 re-processing"
				);
				value.insert(
					"event_id".to_owned(),
					CanonicalJsonValue::String(event_id.as_str().to_owned()),
				);
				check_room_id(room_id, &pdu_event)?;
				return Ok((pdu_event, value));
			}
			value
		},
		| Ok(ruma::signatures::Verified::Signatures) => {
			if let Ok(pdu_event) = self.services.timeline.get_pdu(event_id).await {
				debug!(
					"Received a redacted copy of {event_id}, but we already knew about it. \
					 Re-using known content instead."
				);
				check_room_id(room_id, &pdu_event)?;
				let obj = pdu_event.to_canonical_object();
				return Ok((pdu_event, obj));
			}

			debug_info!("Calculated hash does not match (redaction): {event_id}");
			redact(value, &room_version_rules.redaction, None)
				.map_err(|e| err!(Request(BadJson("Failed to redact {event_id}: {e}"))))?
		},
		| Err(e) => {
			return Err!(Request(Forbidden(debug_error!(
				"Signature verification failed for {event_id}: {e}"
			))));
		},
	};

	// Now that we have checked the signature and hashes we can add the eventID and
	// convert to our PduEvent type
	incoming_pdu
		.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.as_str().to_owned()));

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
			return Err!(Request(Forbidden("Event has rejected auth event: {aid}")));
		}

		if let Ok(auth_event) = self.services.timeline.get_pdu(aid).await {
			check_room_id(room_id, &auth_event)?;
			trace!("Found auth event {aid} for outlier event {event_id} locally");
			auth_events.insert(aid.to_owned(), auth_event);
		} else {
			debug_warn!("Could not find auth event {aid} for outlier event {event_id} locally");
		}
	}

	// Fetch any missing ones & reject invalid ones
	if auth_events.len() != pdu_event.auth_events().count() {
		info!("Missing some auth events, asking remote for auth chain");
		let response: get_event_authorization::v1::Response = self
			.services
			.sending
			.send_federation_request(
				origin,
				get_event_authorization::v1::Request::new(
					room_id.to_owned(),
					event_id.to_owned(),
				),
			)
			.await
			.map_err(|e| {
				err!(Request(Forbidden(
					"Remote server is not divulging incoming event's auth chain: {e}"
				)))
			})?;
		let mut auth_chain_map = HashMap::with_capacity(response.auth_chain.len());
		for auth_pdu_json in response.auth_chain {
			let (auth_event_room_id, auth_event_id, auth_pdu_json) =
				self.parse_incoming_pdu(&auth_pdu_json).await?;
			if auth_event_room_id != room_id {
				return Err!(Request(Forbidden(
					"Auth event {auth_event_id} is in {auth_event_room_id}, not {room_id}."
				)));
			}
			let auth_pdu = PduEvent::from_id_val(&auth_event_id, auth_pdu_json)
				.map_err(|e| err!(Request(BadJson("Invalid PDU {auth_event_id}: {e}"))))?;
			auth_chain_map.insert(auth_event_id, auth_pdu);
		}
		for aid in pdu_event.auth_events() {
			if auth_events.contains_key(aid) {
				continue;
			}
			if let Some(auth_event) = auth_chain_map.get(aid) {
				auth_events.insert(aid.to_owned(), auth_event.clone());
			} else {
				return Err!(Request(Forbidden(
					"Remote server is not divulging incoming event's auth events (missing: \
					 {aid})"
				)));
			}
		}
		// TODO: do events received from auth chain need persisting? that sounds
		// awfully slow
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
				return Err!(Request(Forbidden(
					"Auth event's type and state_key combination exists multiple times: {}, {}",
					auth_event.kind,
					auth_event.state_key().unwrap_or("")
				)));
			},
		}
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
