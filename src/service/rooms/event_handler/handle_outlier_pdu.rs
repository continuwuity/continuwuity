use std::collections::{BTreeMap, HashMap, hash_map};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_info, debug_warn, err, implement, state_res,
	trace, warn,
};
use futures::future::ready;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, RoomId, ServerName,
	events::StateEventType,
};

use super::{check_room_id, get_room_version_id, to_room_version};
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
	auth_events_known: bool,
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

	// TODO: For RoomVersion6 we must check that Raw<..> is canonical do we anywhere?: https://matrix.org/docs/spec/rooms/v6#canonical-json

	// 2. Check signatures, otherwise drop
	// 3. check content hash, redact if doesn't match
	let room_version_id = get_room_version_id(create_event)?;
	let mut incoming_pdu = match self
		.services
		.server_keys
		.verify_event(&value, Some(&room_version_id))
		.await
	{
		| Ok(ruma::signatures::Verified::All) => value,
		| Ok(ruma::signatures::Verified::Signatures) => {
			// Redact
			debug_info!("Calculated hash does not match (redaction): {event_id}");
			let Ok(obj) = ruma::canonical_json::redact(value, &room_version_id, None) else {
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
		if let Ok(auth_event) = self.services.timeline.get_pdu(aid).await {
			check_room_id(room_id, &auth_event)?;
			trace!("Found auth event {aid} for outlier event {event_id} locally");
			auth_events.insert(aid.to_owned(), auth_event);
		} else {
			debug_warn!("Could not find auth event {aid} for outlier event {event_id} locally");
		}
	}

	// Fetch any missing ones & reject invalid ones
	let missing_auth_events = if auth_events_known {
		pdu_event
			.auth_events()
			.filter(|id| !auth_events.contains_key(*id))
			.collect::<Vec<_>>()
	} else {
		pdu_event.auth_events().collect::<Vec<_>>()
	};
	if !missing_auth_events.is_empty() || !auth_events_known {
		debug_info!(
			"Fetching {} missing auth events for outlier event {event_id}",
			missing_auth_events.len()
		);
		for (pdu, _) in self
			.fetch_and_handle_outliers(
				origin,
				missing_auth_events.iter().copied(),
				create_event,
				room_id,
			)
			.await
		{
			auth_events.insert(pdu.event_id().to_owned(), pdu);
		}
	} else {
		debug!("No missing auth events for outlier event {event_id}");
	}
	// reject if we are still missing some
	let still_missing = pdu_event
		.auth_events()
		.filter(|id| !auth_events.contains_key(*id))
		.collect::<Vec<_>>();
	if !still_missing.is_empty() {
		return Err!(Request(InvalidParam(
			"Could not fetch all auth events for outlier event {event_id}, still missing: \
			 {still_missing:?}"
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
				return Err!(Request(InvalidParam(
					"Auth event's type and state_key combination exists multiple times.",
				)));
			},
		}
	}

	// The original create event must be in the auth events
	if !matches!(
		auth_events_by_key.get(&(StateEventType::RoomCreate, String::new().into())),
		Some(_) | None
	) {
		return Err!(Request(InvalidParam("Incoming event refers to wrong create event.")));
	}

	let state_fetch = |ty: &StateEventType, sk: &str| {
		let key = (ty.to_owned(), sk.into());
		ready(auth_events_by_key.get(&key).map(ToOwned::to_owned))
	};

	let auth_check = state_res::event_auth::auth_check(
		&to_room_version(&room_version_id),
		&pdu_event,
		state_fetch,
		create_event.as_pdu(),
	)
	.await
	.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;

	if !auth_check {
		return Err!(Request(Forbidden("Auth check failed")));
	}

	trace!("Validation successful.");

	// 7. Persist the event as an outlier.
	self.services
		.outlier
		.add_pdu_outlier(pdu_event.event_id(), &incoming_pdu);

	trace!("Added pdu as outlier.");

	Ok((pdu_event, incoming_pdu))
}
