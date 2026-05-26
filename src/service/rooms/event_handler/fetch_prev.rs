use std::collections::HashMap;

use conduwuit::{
	Event, PduEvent, debug, debug_info,
	utils::{BoolExt, IterStream, math::try_into, stream::BroadbandExt},
	warn,
};
use futures::StreamExt;
use ruma::{RoomId, ServerName, UInt};

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

		let gapfilled = self
			.get_missing_events(
				room_id,
				incoming_pdu,
				tail,
				origin,
				self.services
					.metadata
					.get_mindepth(room_id)
					.await
					.saturating_sub(
						u8::try_from(incoming_pdu.prev_events.len())
							.unwrap()
							.saturating_mul(2)
							.into(),
					),
			)
			.await?;
		debug_info!("Fetched {} missing events", gapfilled.len());

		// Persist all fetched events
		let mapped = gapfilled
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
			let obj = mapped.get(&event_id).cloned().unwrap();
			match self
				.handle_outlier_pdu(origin, create_event, &event_id, room_id, obj, false)
				.await
			{
				| Ok((pdu, val)) =>
					self.upgrade_outlier_to_timeline_pdu(pdu, val, create_event, origin, room_id)
						.await,
				| Err(e) => {
					warn!("Failed to persist prev_event {event_id}: {e}");
					continue;
				},
			}?;
		}

		// NOTE because i keep forgetting: the caller persists incoming_pdu.
		// we only care about its prev events
		Ok(())
	}
}
