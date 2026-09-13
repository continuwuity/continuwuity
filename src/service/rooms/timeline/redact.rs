use conduwuit_core::{
	Result, err,
	matrix::event::Event,
	utils::{self},
	warn,
};
use ruma::{EventId, events::room::encrypted::Relation};

use super::{ExtractBody, ExtractRelatesTo};
use crate::rooms::short::ShortRoomId;

impl super::Service {
	/// Replace a PDU with the redacted form.
	#[tracing::instrument(name = "redact", level = "debug", skip(self))]
	pub async fn redact_pdu<Pdu: Event + Send + Sync>(
		&self,
		event_id: &EventId,
		reason: &Pdu,
		shortroomid: ShortRoomId,
	) -> Result {
		// TODO: Don't reserialize, keep original json
		let Ok(pdu_id) = self.get_pdu_id(event_id).await else {
			// If event does not exist, just noop
			return Ok(());
		};

		let mut pdu = self
			.get_pdu_from_id(&pdu_id)
			.await
			.map(Event::into_pdu)
			.map_err(|e| {
				err!(Database(error!(?pdu_id, %event_id, ?e, "PDU ID points to invalid PDU.")))
			})?;

		let thread_root = match pdu.get_content::<ExtractRelatesTo>() {
			| Ok(ExtractRelatesTo { relates_to: Relation::Thread(thread) }) =>
				Some(thread.event_id),
			| _ => None,
		};

		if let Ok(content) = pdu.get_content::<ExtractBody>() {
			if let Some(body) = content.body {
				self.services
					.search
					.deindex_pdu(shortroomid, &pdu_id, &body);
			}
		}

		let room_version_id = self
			.services
			.state
			.get_room_version(&pdu.room_id_or_hash())
			.await?;

		if !pdu.is_redacted()
			&& self
				.services
				.config
				.allow_moderators_to_view_redacted_content
		{
			self.db.save_unredacted_content(&pdu_id, &pdu.content);
		}

		pdu.redact(&room_version_id, Some(reason.event_id()))?;

		let obj = utils::to_canonical_object(&pdu).map_err(|e| {
			err!(Database(error!(%event_id, ?e, "Failed to convert PDU to canonical JSON")))
		})?;

		self.replace_pdu(&pdu_id, &obj).await?;

		// So the thread's root doesn't keep the original content in its
		// `latest_event` aggregation
		if let Some(thread_root) = thread_root {
			self.services
				.threads
				.remove_from_thread(&thread_root, &pdu)
				.await
				.inspect_err(|e| {
					warn!("Failed to remove redacted event from thread {thread_root}: {e:?}");
				})
				.ok();
		}

		Ok(())
	}
}
