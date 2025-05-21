use std::sync::Arc;

use conduwuit::{Result, implement};
use database::{Database, Deserialized, Map};
use ruma::{RoomId, UserId};

use crate::{Dep, globals, rooms, rooms::short::ShortStateHash};

pub struct Service {
	db: Data,
	services: Services,
}

struct Data {
	db: Arc<Database>,
	userroomid_notificationcount: Arc<Map>,
	userroomid_highlightcount: Arc<Map>,
	roomuserid_lastnotificationread: Arc<Map>,
	roomsynctoken_shortstatehash: Arc<Map>,
}

struct Services {
	globals: Dep<globals::Service>,
	short: Dep<rooms::short::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				db: args.db.clone(),
				userroomid_notificationcount: args.db["userroomid_notificationcount"].clone(),
				userroomid_highlightcount: args.db["userroomid_highlightcount"].clone(),
				roomuserid_lastnotificationread: args.db["userroomid_highlightcount"].clone(),
				roomsynctoken_shortstatehash: args.db["roomsynctoken_shortstatehash"].clone(),
			},

			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				short: args.depend::<rooms::short::Service>("rooms::short"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

#[implement(Service)]
pub fn reset_notification_counts(&self, user_id: &UserId, room_id: &RoomId) {
	let userroom_id = (user_id, room_id);
	self.db.userroomid_highlightcount.put(userroom_id, 0_u64);
	self.db.userroomid_notificationcount.put(userroom_id, 0_u64);

	let roomuser_id = (room_id, user_id);
	let count = self.services.globals.next_count().unwrap();
	self.db
		.roomuserid_lastnotificationread
		.put(roomuser_id, count);
}

#[implement(Service)]
pub async fn notification_count(&self, user_id: &UserId, room_id: &RoomId) -> u64 {
	let key = (user_id, room_id);
	self.db
		.userroomid_notificationcount
		.qry(&key)
		.await
		.deserialized()
		.unwrap_or(0)
}

#[implement(Service)]
pub async fn highlight_count(&self, user_id: &UserId, room_id: &RoomId) -> u64 {
	let key = (user_id, room_id);
	self.db
		.userroomid_highlightcount
		.qry(&key)
		.await
		.deserialized()
		.unwrap_or(0)
}

#[implement(Service)]
pub async fn last_notification_read(&self, user_id: &UserId, room_id: &RoomId) -> u64 {
	let key = (room_id, user_id);
	self.db
		.roomuserid_lastnotificationread
		.qry(&key)
		.await
		.deserialized()
		.unwrap_or(0)
}

#[implement(Service)]
pub async fn associate_token_shortstatehash(
	&self,
	room_id: &RoomId,
	token: u64,
	shortstatehash: ShortStateHash,
) {
	let shortroomid = self
		.services
		.short
		.get_shortroomid(room_id)
		.await
		.expect("room exists");

	let _cork = self.db.db.cork();
	let key: &[u64] = &[shortroomid, token];
	self.db
		.roomsynctoken_shortstatehash
		.put(key, shortstatehash);
}

#[implement(Service)]
pub async fn get_token_shortstatehash(
	&self,
	room_id: &RoomId,
	token: u64,
) -> Result<ShortStateHash> {
	let shortroomid = self.services.short.get_shortroomid(room_id).await?;

	let key: &[u64] = &[shortroomid, token];
	self.db
		.roomsynctoken_shortstatehash
		.qry(key)
		.await
		.deserialized()
}

/// Count how many sync tokens exist for a room without deleting them
///
/// This is useful for dry runs to see how many tokens would be deleted
#[implement(Service)]
pub async fn count_room_tokens(&self, room_id: &RoomId) -> Result<usize> {
	use futures::TryStreamExt;

	let shortroomid = self.services.short.get_shortroomid(room_id).await?;

	// Create a prefix to search by - all entries for this room will start with its
	// short ID
	let prefix = &[shortroomid];

	// Collect all keys into a Vec and count them
	let keys = self
		.db
		.roomsynctoken_shortstatehash
		.keys_prefix_raw(prefix)
		.map_ok(|_| ()) // We only need to count, not store the keys
		.try_collect::<Vec<_>>()
		.await?;

	Ok(keys.len())
}

/// Delete all sync tokens associated with a room
///
/// This helps clean up the database as these tokens are never otherwise removed
#[implement(Service)]
pub async fn delete_room_tokens(&self, room_id: &RoomId) -> Result<usize> {
	use futures::TryStreamExt;

	let shortroomid = self.services.short.get_shortroomid(room_id).await?;

	// Create a prefix to search by - all entries for this room will start with its
	// short ID
	let prefix = &[shortroomid];

	// Get all keys with this room prefix
	let mut count = 0;

	// Collect all keys into a Vec first, then delete them
	let keys = self
		.db
		.roomsynctoken_shortstatehash
		.keys_prefix_raw(prefix)
		.map_ok(|key| {
			// Clone the key since we can't store references in the Vec
			Vec::from(key)
		})
		.try_collect::<Vec<_>>()
		.await?;

	// Delete each key individually
	for key in &keys {
		self.db.roomsynctoken_shortstatehash.del(key);
		count += 1;
	}

	Ok(count)
}
