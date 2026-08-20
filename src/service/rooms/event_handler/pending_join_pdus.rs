use conduwuit::{debug, debug_warn};
use ruma::{CanonicalJsonObject, RoomId, ServerName};
use serde_json::value::RawValue as RawJsonValue;

impl super::Service {
	pub fn begin_remote_join(&self, room_id: &RoomId) {
		let (tx, mut rx) = tokio::sync::mpsc::channel(
			self.services.server.config.remote_join_pdu_queue_capacity,
		);
		self.joining_rooms.write().insert(room_id.to_owned(), tx);
		let Some(service) = self.me.upgrade() else {
			return;
		};
		let room_id = room_id.to_owned();

		self.services.server.runtime().spawn(async move {
			let mut pdus = Vec::new();
			let mut complete = false;
			while let Some(pdu) = rx.recv().await {
				match pdu {
					| super::PendingJoinPdu::Pdu(origin, pdu) => pdus.push((origin, pdu)),
					| super::PendingJoinPdu::Complete => {
						complete = true;
						break;
					},
				}
			}
			if !complete {
				return;
			}
			let _room_lock = service.mutex_federation.lock(room_id.as_str()).await;

			for (origin, pdu) in pdus {
				let Ok((room_id, event_id, value)) = service.parse_incoming_pdu(&pdu, None).await
				else {
					debug_warn!("Failed to parse PDU queued during remote join");
					continue;
				};
				if let Err(error) = service
					.handle_incoming_pdu(&origin, &room_id, &event_id, value, false)
					.await
				{
					debug_warn!(%room_id, %event_id, "Failed to process PDU queued during remote join: {error}");
				}
			}
		});
	}

	pub fn cancel_remote_join(&self, room_id: &RoomId) {
		self.joining_rooms.write().remove(room_id);
	}

	pub fn queue_pending_join_pdu(&self, origin: &ServerName, pdu: Box<RawJsonValue>) -> bool {
		let Ok(value) = serde_json::from_str::<CanonicalJsonObject>(pdu.get()) else {
			return false;
		};
		let Some(room_id) = value.get("room_id").and_then(|value| value.as_str()) else {
			return false;
		};
		let Ok(room_id) = RoomId::parse(room_id) else {
			return false;
		};
		let joining_rooms = self.joining_rooms.read();
		let Some(tx) = joining_rooms.get(&room_id) else {
			return false;
		};
		let pdu = super::PendingJoinPdu::Pdu(origin.to_owned(), pdu);
		if tx.try_send(pdu).is_err() {
			debug_warn!(%room_id, "Dropping PDU received during remote join because the queue is full");
		} else {
			debug!(%room_id, "Queued PDU received during remote join");
		}
		true
	}

	pub async fn process_pending_join_pdus(&self, room_id: &RoomId) {
		let Some(tx) = self.joining_rooms.write().remove(room_id) else {
			return;
		};
		let _ = tx.send(super::PendingJoinPdu::Complete).await;
	}
}
