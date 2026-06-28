use std::sync::Arc;

use conduwuit::Result;
use database::{Deserialized, Map};
use ruma::{RoomId, UserId};

use crate::{Dep, globals};

pub struct Service {
	db: Data,
	services: Services,
}

struct Data {
	userroomid_notificationcount: Arc<Map>,
	userroomid_highlightcount: Arc<Map>,
	roomuserid_lastnotificationread: Arc<Map>,
}

struct Services {
	globals: Dep<globals::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				userroomid_notificationcount: args.db["userroomid_notificationcount"].clone(),
				userroomid_highlightcount: args.db["userroomid_highlightcount"].clone(),
				roomuserid_lastnotificationread: args.db["userroomid_highlightcount"].clone(),
			},
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Resets the notification counts for a room the user is in.
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

	/// Gets the notification count for a room the user is in.
	pub async fn notification_count(&self, user_id: &UserId, room_id: &RoomId) -> u64 {
		let key = (user_id, room_id);
		self.db
			.userroomid_notificationcount
			.qry(&key)
			.await
			.deserialized()
			.unwrap_or(0)
	}

	/// Gets the number of events that highlighted the user in a given room.
	/// These aren't necessarily notifications.
	pub async fn highlight_count(&self, user_id: &UserId, room_id: &RoomId) -> u64 {
		let key = (user_id, room_id);
		self.db
			.userroomid_highlightcount
			.qry(&key)
			.await
			.deserialized()
			.unwrap_or(0)
	}

	/// Returns the last notification the user read in the room, or 0 if none.
	pub async fn last_notification_read(&self, user_id: &UserId, room_id: &RoomId) -> u64 {
		let key = (room_id, user_id);
		self.db
			.roomuserid_lastnotificationread
			.qry(&key)
			.await
			.deserialized()
			.unwrap_or(0)
	}
}
