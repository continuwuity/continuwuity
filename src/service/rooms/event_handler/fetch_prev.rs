use std::collections::{HashMap, HashSet};

use conduwuit::{
	Event, PduEvent, debug, debug_info,
	result::DebugInspect,
	utils::{BoolExt, IterStream, stream::BroadbandExt},
};
use futures::StreamExt;
use ruma::{RoomId, ServerName};

use crate::rooms::event_handler::build_local_dag;

impl super::Service {
	/// Fetches any missing prev_events for this event and persists them before
	/// returning.
	pub(super) async fn fetch_prevs(
		&self,
		room_id: &RoomId,
		create_event: &PduEvent,
		incoming_pdu: &PduEvent,
		origin: &ServerName,
	) -> conduwuit::Result<()> {
		let missing = incoming_pdu
			.prev_events()
			.stream()
			.broad_filter_map(|event_id| async move {
				self.services
					.timeline
					.get_non_outlier_pdu_json(event_id)
					.await
					.inspect(|_| debug!("Found prev_event {event_id} locally."))
					.inspect_err(|e| debug!(%e, "Could not find prev_event {event_id} locally."))
					.is_ok()
					.or(|| event_id.to_owned())
			})
			.collect::<Vec<_>>()
			.await;
		if missing.is_empty() {
			debug!(event_id=%incoming_pdu.event_id(), "No missing prev events.");
			return Ok(());
		}
		debug!(%room_id, event_id=%incoming_pdu.event_id(), ?missing, "Fetching previous events");
		let tail = self
			.services
			.state
			.get_forward_extremities(room_id)
			.collect::<Vec<_>>()
			.await;

		let backfilled = self
			.backfill_missing_events(
				room_id.to_owned(),
				HashSet::from_iter(vec![incoming_pdu.event_id().to_owned()]),
				tail,
				origin.to_owned(),
			)
			.await?;
		debug_info!("Fetched {} missing events", backfilled.len());

		// Persist all fetched events
		let mapped = backfilled
			.iter()
			.map(|(eid, evt)| {
				let mut obj = evt.to_canonical_object();
				obj.remove("event_id"); // event_id is inserted by backfill_missing_events
				(eid.clone(), obj)
			})
			.collect::<HashMap<_, _>>();

		let to_persist = if mapped.len() <= 1 {
			mapped.keys().map(ToOwned::to_owned).collect()
		} else {
			build_local_dag(&mapped).await?
		};

		for event_id in to_persist {
			debug_info!("Persisting fetched prev event {event_id}");
			let pdu_event = backfilled.get(&event_id).cloned().unwrap_or_else(|| {
				panic!("Event {event_id} was in backfill response but not in map")
			});
			let obj = pdu_event.to_canonical_object();
			self.upgrade_outlier_to_timeline_pdu(pdu_event, obj, create_event, origin, room_id)
				.await
				.debug_inspect(|_| debug_info!("Persisted fetched prev event {event_id}"))?;
		}

		// NOTE because i keep forgetting: the caller persists incoming_pdu.
		// we only care about its prev events
		Ok(())
	}
}
