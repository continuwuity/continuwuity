use std::collections::{HashMap, hash_map};

use conduwuit::{Err, Event, EventTypeExt, PduEvent, Result, err, warn};
use ruma::{
	OwnedEventId, ServerName, api::federation::authorization::get_event_authorization,
	room_version_rules::RoomVersionRules,
};
use tokio::join;

use crate::rooms::event_handler::{build_local_dag, fetch_and_handle_outliers::DagBuilderTree};

impl super::Service {
	/// Fetches (and persists) the incoming event's entire auth chain by asking
	/// the remote server for it. The events are added to the outlier tree, but
	/// no de-outliering is attempted.
	///
	/// Returns a map of auth events, which includes potentially unauthorised
	/// ones - it is the caller's responsibility to check rejection status of
	/// each required event post factum.
	#[tracing::instrument(skip_all)]
	pub(super) async fn fetch_and_persist_event_auth<Pdu>(
		&self,
		incoming_event: &PduEvent,
		origin: &ServerName,
		room_version_rules: &RoomVersionRules,
		create_event: &Pdu,
	) -> Result<HashMap<OwnedEventId, PduEvent>>
	where
		Pdu: Event + Send + Sync,
	{
		let event_auth: get_event_authorization::v1::Response = self
			.services
			.sending
			.send_federation_request(
				origin,
				get_event_authorization::v1::Request::new(
					incoming_event.room_id_or_hash().clone(),
					incoming_event.event_id().to_owned(),
				),
			)
			.await
			.map_err(|e| {
				err!(Request(Forbidden(
					"Remote server is not divulging incoming event's auth chain: {e}"
				)))
			})?;

		let mut auth_chain_map = HashMap::with_capacity(event_auth.auth_chain.len());

		for auth_pdu_json in event_auth.auth_chain {
			let (auth_event_room_id, auth_event_id, auth_pdu_json) =
				match self.parse_incoming_pdu(&auth_pdu_json).await {
					| Ok(parsed) => parsed,
					| Err(e) => {
						warn!(error=?e, "Dropping auth chain event as it could not be parsed");
						continue;
					},
				};
			if let Err(e) = Self::pdu_format_check_1(
				&auth_pdu_json,
				room_version_rules,
				create_event.event_id(),
			) {
				// drop this PDU
				warn!(%auth_event_id, error=?e, "Dropping auth chain event as it violates the room event format");
				continue;
			}
			let auth_pdu_json = match self
				.signature_hash_check_2_3(auth_pdu_json, room_version_rules)
				.await
			{
				| Ok(pdu_json) => pdu_json,
				| Err(e) => {
					// drop this PDU
					warn!(
						%auth_event_id,
						error=?e,
						"Dropping auth chain event as it has an invalid signature"
					);
					continue;
				},
			};

			// PDU check 4 is done when we've finished aggregating
			if auth_event_room_id != incoming_event.room_id_or_hash() {
				return Err!(Request(Forbidden(
					"Auth chain event {auth_event_id} is in {auth_event_room_id}, not {}.",
					incoming_event.room_id_or_hash()
				)));
			}
			let auth_pdu = PduEvent::from_id_val(&auth_event_id, auth_pdu_json).map_err(|e| {
				err!(Request(BadJson("Invalid PDU {auth_event_id} in auth chain: {e}")))
			})?;

			if auth_pdu.state_key().is_none() {
				return Err!(Request(BadJson(
					"Invalid PDU {auth_event_id} in auth_chain: not a state event"
				)));
			}

			auth_chain_map.insert(auth_event_id, auth_pdu);
		}

		// We need to authorise each returned PDU to make sure that the caller can
		// correctly detect if one of them is rejected. We don't remove the event from
		// the map in case the caller incorrectly then identifies that event as missing,
		// rather than rejected.
		self.authorise_remote_auth_chain(
			&auth_chain_map,
			room_version_rules,
			create_event.as_pdu(),
		)
		.await?;

		Ok(auth_chain_map)
	}

	/// Runs through the returned auth chain topologically and flags any events
	/// that fail PDU check 4.
	///
	/// An empty `Ok` is returned unless the auth chain is missing required
	/// events (which means the remote server returned an invalid response).
	async fn authorise_remote_auth_chain<Pdu>(
		&self,
		auth_chain_map: &HashMap<OwnedEventId, PduEvent>,
		room_version_rules: &RoomVersionRules,
		create_event: &Pdu,
	) -> Result<()>
	where
		Pdu: Event + Send + Sync,
	{
		let pdu_objects = auth_chain_map
			.iter()
			.map(|(event_id, pdu)| (event_id.clone(), pdu.to_canonical_object()))
			.collect::<HashMap<_, _>>();
		let auth_chain_topo = build_local_dag(&pdu_objects, DagBuilderTree::AuthEvents)
			.await?
			.into_iter()
			.map(|event_id| (event_id.clone(), auth_chain_map.get(&event_id).unwrap().to_owned()))
			.collect::<HashMap<_, _>>();

		'outer: for (event_id, pdu) in auth_chain_topo {
			// If we know the event is rejected OR have it locally, we can skip this check.
			// This is safe because if we know the event is rejected, running event auth
			// will just reject it again. And, if we have it locally, and HAVEN'T flagged it
			// as rejected, then we know it was at least accepted under the check we would
			// otherwise be about to perform, previously.
			let (is_rejected, have_locally) = join!(
				self.services.pdu_metadata.is_event_rejected(&event_id),
				self.services.timeline.pdu_exists(&event_id)
			);
			if is_rejected || have_locally {
				continue;
			}

			// IMPORTANT: We can't use the handy dandy `handle_outlier_pdu` function here
			// because it may then try to fetch missing auth events, resulting in deep
			// recursion. We will do the minimum required steps to validate the PDU here.
			// Checks 1-3 were already done before this function is called, so we only need
			// to do check 4.

			let mut auth_events_by_key: HashMap<_, _> =
				HashMap::with_capacity(pdu.auth_events.len());

			for auth_event_id in pdu.auth_events() {
				let Some(auth_event) = auth_chain_map.get(auth_event_id) else {
					return Err!(Request(NotFound(
						"Invalid event_auth response from remote server: {event_id} depends on \
						 {auth_event_id}, but it is not present"
					)));
				};

				self.services
					.outlier
					.add_pdu_outlier(auth_event_id, &auth_event.to_canonical_object());
				let key = auth_event
					.kind()
					.with_state_key(auth_event.state_key().unwrap());
				match auth_events_by_key.entry(key) {
					| hash_map::Entry::Vacant(v) => {
						v.insert(auth_event.clone());
					},
					| hash_map::Entry::Occupied(_) => {
						// Duplicate auth events by key are not allowed.
						self.reject_and_persist(&event_id, &pdu.to_canonical_object());
						continue 'outer;
					},
				}
			}
			if !self
				.auth_state_check_4(
					&pdu,
					room_version_rules,
					create_event.as_pdu(),
					&auth_events_by_key,
				)
				.await?
			{
				self.reject_and_persist(&event_id, &pdu.to_canonical_object());
			}
		}

		Ok(())
	}
}
