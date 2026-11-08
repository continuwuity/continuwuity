use std::sync::Arc;

use conduwuit::{
	Result,
	utils::{ReadyExt, stream::TryIgnore},
};
use database::{Deserialized, Json, Map};
use futures::{Stream, StreamExt};
use ruma::{
	CanonicalJsonObject, OwnedRoomId, OwnedUserId, RoomId, UserId,
	events::{AnySyncEphemeralRoomEvent, receipt::ReceiptEvent},
	serde::Raw,
};

use crate::{Dep, globals};

pub(super) struct Data {
	roomuserid_privateread: Arc<Map>,
	roomuserid_lastprivatereadupdate: Arc<Map>,
	services: Services,
	readreceiptid_readreceipt: Arc<Map>,
}

struct Services {
	globals: Dep<globals::Service>,
}

pub(super) type ReceiptItem = (OwnedUserId, u64, Raw<AnySyncEphemeralRoomEvent>);

impl Data {
	pub(super) fn new(args: &crate::Args<'_>) -> Self {
		let db = &args.db;
		Self {
			roomuserid_privateread: db["roomuserid_privateread"].clone(),
			roomuserid_lastprivatereadupdate: db["roomuserid_lastprivatereadupdate"].clone(),
			readreceiptid_readreceipt: db["readreceiptid_readreceipt"].clone(),
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
			},
		}
	}

	pub(super) async fn readreceipt_update(
		&self,
		user_id: &UserId,
		room_id: &RoomId,
		event: &ReceiptEvent,
	) -> bool {
		let new_target = event.content.0.iter().find_map(|(ev_id, types)| {
			types
				.iter()
				.find_map(|(rtype, users)| users.get(user_id).map(|r| (ev_id, rtype, &r.thread)))
		});

		// Find old entries for this user in this room
		let last_possible_key = (room_id, u64::MAX);
		let mut keys = Vec::new();
		self.readreceiptid_readreceipt
			.rev_keys_from_raw(&last_possible_key)
			.ignore_err()
			.ready_take_while(|key| key.starts_with(room_id.as_bytes()))
			.ready_filter_map(|key| key.ends_with(user_id.as_bytes()).then(|| key.to_vec()))
			.ready_for_each(|key| keys.push(key))
			.await;

		let mut to_del = Vec::new();
		let mut is_dup = false;

		for key in keys {
			if let Ok(val) = self.readreceiptid_readreceipt.get(&key).await {
				if let Ok(existing) = serde_json::from_slice::<ReceiptEvent>(&val) {
					let existing_target = existing.content.0.iter().find_map(|(ev_id, types)| {
						types.iter().find_map(|(rtype, users)| {
							users.get(user_id).map(|r| (ev_id, rtype, &r.thread))
						})
					});

					if let (Some(new_t), Some(old_t)) = (new_target, existing_target) {
						if new_t.1 == old_t.1 && new_t.2 == old_t.2 {
							if new_t.0 == old_t.0 {
								is_dup = true;
							} else {
								to_del.push(key);
							}
						}
					} else {
						to_del.push(key);
					}
				} else {
					to_del.push(key);
				}
			} else {
				to_del.push(key);
			}
		}

		if is_dup {
			return false;
		}

		for key in to_del {
			self.readreceiptid_readreceipt.del(&key);
		}

		let count = self.services.globals.next_count().unwrap();
		let latest_id = (room_id, count, user_id);
		self.readreceiptid_readreceipt.put(latest_id, Json(event));
		true
	}

	pub(super) fn readreceipts_since<'a>(
		&'a self,
		room_id: &'a RoomId,
		since: u64,
	) -> impl Stream<Item = ReceiptItem> + Send + 'a {
		type Key = (OwnedRoomId, u64, OwnedUserId);
		type KeyVal = (Key, CanonicalJsonObject);

		let after_since = since.saturating_add(1); // +1 so we don't send the event at since
		let first_possible_edu = (room_id, after_since);

		self.readreceiptid_readreceipt
			.stream_from(&first_possible_edu)
			.ignore_err()
			.ready_take_while(move |((r, ..), _): &KeyVal| *r == room_id)
			.map(move |((_, count, user_id), mut json): KeyVal| {
				json.remove("room_id");

				let event = serde_json::value::to_raw_value(&json)?;

				Ok((user_id, count, Raw::from_json(event)))
			})
			.ignore_err()
	}

	pub(super) async fn private_read_set(
		&self,
		room_id: &RoomId,
		user_id: &UserId,
		pdu_count: u64,
	) -> bool {
		let key = (room_id, user_id);
		if let Ok(current) = self
			.roomuserid_privateread
			.qry(&key)
			.await
			.deserialized::<u64>()
		{
			if current == pdu_count {
				return false;
			}
		}

		let next_count = self.services.globals.next_count().unwrap();
		self.roomuserid_privateread.put(key, pdu_count);
		self.roomuserid_lastprivatereadupdate.put(key, next_count);
		true
	}

	pub(super) async fn private_read_get_count(
		&self,
		room_id: &RoomId,
		user_id: &UserId,
	) -> Result<u64> {
		let key = (room_id, user_id);
		self.roomuserid_privateread.qry(&key).await.deserialized()
	}

	pub(super) async fn last_privateread_update(
		&self,
		user_id: &UserId,
		room_id: &RoomId,
	) -> u64 {
		let key = (room_id, user_id);
		self.roomuserid_lastprivatereadupdate
			.qry(&key)
			.await
			.deserialized()
			.unwrap_or(0)
	}
}
