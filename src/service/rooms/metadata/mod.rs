use std::sync::Arc;

use conduwuit::{Result, utils::stream::TryIgnore};
use database::{Deserialized, Map};
use futures::{Stream, StreamExt};
use ruma::{OwnedRoomId, RoomId, UInt, uint};

use crate::{Dep, rooms};

pub struct Service {
	db: Data,
	services: Services,
}

struct Data {
	disabledroomids: Arc<Map>,
	bannedroomids: Arc<Map>,
	roomid_shortroomid: Arc<Map>,
	pduid_pdu: Arc<Map>,
	roomid_mindepth: Arc<Map>,
}

struct Services {
	short: Dep<rooms::short::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				disabledroomids: args.db["disabledroomids"].clone(),
				bannedroomids: args.db["bannedroomids"].clone(),
				roomid_shortroomid: args.db["roomid_shortroomid"].clone(),
				pduid_pdu: args.db["pduid_pdu"].clone(),
				roomid_mindepth: args.db["roomid_mindepth"].clone(),
			},
			services: Services {
				short: args.depend::<rooms::short::Service>("rooms::short"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Checks if a room exists by checking if there are any PDUs in that room.
	/// If the short room ID is not found or there are no PDUs in the room, it
	/// does not exist.
	pub async fn exists(&self, room_id: &RoomId) -> bool {
		let Ok(prefix) = self.services.short.get_shortroomid(room_id).await else {
			return false;
		};

		// Look for PDUs in that room.
		self.db
			.pduid_pdu
			.keys_prefix_raw(&prefix)
			.ignore_err()
			.next()
			.await
			.is_some()
	}

	/// Returns a stream of every room ID in the database.
	pub fn iter_ids(&self) -> impl Stream<Item = OwnedRoomId> + Send + '_ {
		self.db.roomid_shortroomid.keys().ignore_err()
	}

	/// Disables a room, preventing it from engaging in federation, but still
	/// allowing it to be used locally.
	pub fn disable_room(&self, room_id: &RoomId, disabled: bool) {
		if disabled {
			self.db.disabledroomids.insert(room_id, []);
		} else {
			self.db.disabledroomids.remove(room_id);
		}
	}

	pub async fn is_disabled(&self, room_id: &RoomId) -> bool {
		self.db.disabledroomids.get(room_id).await.is_ok()
	}

	/// Bans a room, ensuring it cannot be used neither locally nor over
	/// federation.
	pub fn ban_room(&self, room_id: &RoomId, banned: bool) {
		if banned {
			self.db.bannedroomids.insert(room_id, []);
		} else {
			self.db.bannedroomids.remove(room_id);
		}
	}

	/// Checks if a room is currently banned.
	pub async fn is_banned(&self, room_id: &RoomId) -> bool {
		self.db.bannedroomids.get(room_id).await.is_ok()
	}

	/// Lists all rooms that are currently banned.
	pub fn list_banned_rooms(&self) -> impl Stream<Item = OwnedRoomId> + Send + '_ {
		self.db.bannedroomids.keys().ignore_err()
	}

	pub async fn get_mindepth(&self, room_id: &RoomId) -> UInt {
		self.db
			.roomid_mindepth
			.get(room_id)
			.await
			.deserialized::<UInt>()
			.unwrap_or_else(|_| uint!(0))
	}

	pub fn set_mindepth(&self, room_id: &RoomId, min_depth: u64) {
		self.db
			.roomid_mindepth
			.put_raw(room_id.as_bytes(), min_depth.to_be_bytes());
	}

	pub async fn maybe_set_mindepth(&self, room_id: &RoomId, min_depth: u64) {
		if min_depth > self.get_mindepth(room_id).await.into() {
			self.set_mindepth(room_id, min_depth);
		}
	}
}
