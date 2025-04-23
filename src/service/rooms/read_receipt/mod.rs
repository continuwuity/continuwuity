mod data;

use std::{collections::{BTreeMap, HashMap}, sync::Arc};

use conduwuit::{
	Result, debug, err,
	matrix::pdu::{PduCount, PduId, RawPduId},
	warn,
};
use futures::{Stream, TryFutureExt, try_join};
use ruma::{
	events::{
		receipt::{ReceiptEvent, ReceiptEventContent, ReceiptType, Receipts}, AnySyncEphemeralRoomEvent, SyncEphemeralRoomEvent
	}, serde::Raw, OwnedEventId, OwnedUserId, RoomId, UserId
};

use self::data::{Data, ReceiptItem};
use crate::{Dep, rooms, sending};

pub struct Service {
	services: Services,
	db: Data,
}

struct Services {
	sending: Dep<sending::Service>,
	short: Dep<rooms::short::Service>,
	timeline: Dep<rooms::timeline::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				sending: args.depend::<sending::Service>("sending"),
				short: args.depend::<rooms::short::Service>("rooms::short"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
			},
			db: Data::new(&args),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Updates the public read receipt (`m.read`) based on the incoming event.
	/// If the event referenced by the new public receipt is newer than the current
	/// private read marker (`m.read.private`), the private marker is also updated
	/// to match the public receipt's position.
	pub async fn readreceipt_update(
		&self,
		user_id: &UserId,
		room_id: &RoomId,
		event: &ReceiptEvent,
	) {
		debug!(target: "readreceipt", %room_id, %user_id, "Updating read receipt in database.");

		// 2. Find the maximum PDU count for the m.read event(s) referenced in the new receipt
		let mut max_new_public_pdu_count: Option<PduCount> = None;
		for (event_id, receipts) in event.content.0.iter() {
			// Check if this event_id has an m.read receipt for the target user
			if let Some(user_receipts) = receipts.get(&ReceiptType::Read) {
				if user_receipts.contains_key(user_id) {
					// Try to get the PDU count (timeline position) for this event_id
					match self.services.timeline.get_pdu_count(event_id).await {
						Ok(count) => {
							// Update the maximum count found so far
							let current_max = max_new_public_pdu_count.unwrap_or(PduCount::Normal(0));
							max_new_public_pdu_count = Some(current_max.max(count));
							debug!(target: "readreceipt", %room_id, %user_id, %event_id, count, "Found PDU count for new public receipt event.");
						}
						Err(e) => {
							warn!(
								target: "readreceipt", %room_id, %user_id, %event_id,
								"Failed to get PDU count for event ID from new public read receipt: {}",
								e
							);
						}
					}
				}
			}
		}

		// Flush the sending queue for the room to notify clients
		if let Err(e) = self.services.sending.flush_room(room_id).await {
			warn!(target: "readreceipt", %room_id, %user_id, "Failed to flush room after read receipt update: {}", e);
		}
	}

	/// Gets the latest private read receipt from the user in the room
	pub async fn private_read_get(
		&self,
		room_id: &RoomId,
		user_id: &UserId,
	) -> Result<Raw<AnySyncEphemeralRoomEvent>> {
		let pdu_count = self.private_read_get_count(room_id, user_id).map_err(|e| {
			err!(Database(warn!("No private read receipt was set in {room_id}: {e}")))
		});
		let shortroomid = self.services.short.get_shortroomid(room_id).map_err(|e| {
			err!(Database(warn!("Short room ID does not exist in database for {room_id}: {e}")))
		});
		let (pdu_count, shortroomid) = try_join!(pdu_count, shortroomid)?;

		let shorteventid = PduCount::Normal(pdu_count);
		let pdu_id: RawPduId = PduId { shortroomid, shorteventid }.into();

		let pdu = self.services.timeline.get_pdu_from_id(&pdu_id).await?;

		let event_id: OwnedEventId = pdu.event_id;
		let user_id: OwnedUserId = user_id.to_owned();
		let content: BTreeMap<OwnedEventId, Receipts> = BTreeMap::from_iter([(
			event_id,
			BTreeMap::from_iter([(
				ruma::events::receipt::ReceiptType::ReadPrivate,
				BTreeMap::from_iter([(user_id, ruma::events::receipt::Receipt {
					ts: None, // TODO: start storing the timestamp so we can return one
					thread: ruma::events::receipt::ReceiptThread::Unthreaded,
				})]),
			)]),
		)]);
		let receipt_event_content = ReceiptEventContent(content);
		let receipt_sync_event = SyncEphemeralRoomEvent { content: receipt_event_content };

		let event = serde_json::value::to_raw_value(&receipt_sync_event)
			.expect("receipt created manually");

		Ok(Raw::from_json(event))
	}

	/// Returns an iterator over the most recent read_receipts in a room that
	/// happened after the event with id `since`.
	#[inline]
	#[tracing::instrument(skip(self), level = "debug")]
	pub fn readreceipts_since<'a>(
		&'a self,
		room_id: &'a RoomId,
		since: u64,
	) -> impl Stream<Item = ReceiptItem<'_>> + Send + 'a {
		self.db.readreceipts_since(room_id, since)
	}

	/// Sets a private read marker at PDU `count`.
	#[inline]
	#[tracing::instrument(skip(self), level = "debug")]
	pub fn private_read_set(&self, room_id: &RoomId, user_id: &UserId, count: u64) {
		self.db.private_read_set(room_id, user_id, count);
	}

	/// Returns the private read marker PDU count.
	#[inline]
	#[tracing::instrument(skip(self), level = "debug")]
	pub async fn private_read_get_count(
		&self,
		room_id: &RoomId,
		user_id: &UserId,
	) -> Result<u64> {
		self.db.private_read_get_count(room_id, user_id).await
	}

	/// Returns the PDU count of the last typing update in this room.
	#[inline]
	pub async fn last_privateread_update(&self, user_id: &UserId, room_id: &RoomId) -> u64 {
		self.db.last_privateread_update(user_id, room_id).await
	}
}

#[must_use]
pub fn pack_receipts<I>(receipts: I) -> Raw<SyncEphemeralRoomEvent<ReceiptEventContent>>
where
	I: Iterator<Item = Raw<AnySyncEphemeralRoomEvent>>,
{
	let mut json = BTreeMap::new();
	for value in receipts {
		let receipt = serde_json::from_str::<SyncEphemeralRoomEvent<ReceiptEventContent>>(
			value.json().get(),
		);
		match receipt {
			| Ok(value) =>
				for (event, receipt) in value.content {
					json.insert(event, receipt);
				},
			| _ => {
				debug!("failed to parse receipt: {:?}", receipt);
			},
		}
	}
	let content = ReceiptEventContent::from_iter(json);

	conduwuit::trace!(?content);
	Raw::from_json(
		serde_json::value::to_raw_value(&SyncEphemeralRoomEvent { content })
			.expect("received valid json"),
	)
}
