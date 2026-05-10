use std::collections::{HashMap, HashSet, VecDeque};

use conduwuit::{Event, PduEvent, debug, debug_info, error, trace};
use ruma::{OwnedEventId, RoomId, ServerName};

use crate::rooms::event_handler::build_local_dag;

impl super::Service {
	pub(super) async fn fetch_prevs(
		&self,
		room_id: &RoomId,
		create_event: &PduEvent,
		incoming_pdu: &PduEvent,
		origin: &ServerName,
	) -> conduwuit::Result<()> {
		let mut queue: VecDeque<OwnedEventId> = VecDeque::new();
		queue.push_back(incoming_pdu.event_id().to_owned());

		while let Some(event_id) = queue.pop_front() {
			debug!(event_id=%incoming_pdu.event_id, "Fetching any missing prev_events");
			let mut missing_prev_events: HashSet<OwnedEventId> =
				incoming_pdu.prev_events().map(ToOwned::to_owned).collect();
			for pid in missing_prev_events.clone() {
				if self.services.timeline.pdu_exists(&pid).await {
					trace!("Found prev event {pid} for outlier event {event_id} locally");
					missing_prev_events.remove(&pid);
				} else {
					debug_info!(
						"Could not find prev event {pid} for outlier event {event_id} locally, \
						 will fetch over federation"
					);
				}
			}
			if !missing_prev_events.is_empty() {
				debug_info!(
					"Fetching {} missing prev events for outlier event {event_id}",
					missing_prev_events.len()
				);
				let backfilled = self
					.backfill_missing_events(
						room_id.to_owned(),
						vec![event_id.clone()],
						origin.to_owned(),
					)
					.await?;
				debug_info!("Fetched {} missing events for {event_id}", backfilled.len());
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
				debug_info!("Preparing to handle {} missing events", backfilled.len());
				for prev_event_id in local_dag {
					let obj = mapped
						.get(&prev_event_id)
						.expect("We should have this event in memory");
					debug_info!("Handling prev event {prev_event_id}");
					match self
						.handle_outlier_pdu(
							origin,
							create_event,
							&prev_event_id,
							room_id,
							obj.clone(),
							false,
						)
						.await
					{
						| Ok(_) => {
							debug!("Successfully handled {prev_event_id} as an outlier");
							missing_prev_events.remove(&prev_event_id);
						},
						| Err(e) =>
							error!(error=?e, %prev_event_id, %event_id, "Failed to handle prev event"),
					}
					debug_info!("Finished handling prev");
				}
			}
			let outlier = self.services.timeline.get_pdu(&event_id).await;
			if missing_prev_events.is_empty()
				&& let Ok(pdu) = outlier
			{
				// promote any prevs first
				for prev_event_id in pdu.prev_events() {
					debug_info!("Promoting prev event {prev_event_id} to timeline");
					let prev_pdu = self.services.timeline.get_pdu(&event_id).await?;
					let val = prev_pdu.to_canonical_object();
					self.upgrade_outlier_to_timeline_pdu(
						prev_pdu,
						val,
						create_event,
						origin,
						room_id,
					)
					.await?;
					debug_info!("Finished prev promoting {prev_event_id} to timeline");
				}
				debug_info!("Promoting event {event_id} to timeline");
				let val = pdu.to_canonical_object();
				self.upgrade_outlier_to_timeline_pdu(pdu, val, create_event, origin, room_id)
					.await?;
				debug_info!("Finished promoting {event_id} to timeline");
			} else {
				debug!(?missing_prev_events, ok=%outlier.is_ok(), "Not promoting {event_id}");
			}
		}

		Ok(())
	}
}
