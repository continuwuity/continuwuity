use std::sync::Arc;

use conduwuit::{Result, utils::stream::TryIgnore};
use database::Map;
use futures::Stream;
use ruma::{OwnedRoomId, RoomId, api::client::room::Visibility};

pub struct Service {
	db: Data,
}

struct Data {
	publicroomids: Arc<Map>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				publicroomids: args.db["publicroomids"].clone(),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Marks a room as public, adding it to the room directory. Has no relation
	/// to the join rule.
	pub fn set_public(&self, room_id: &RoomId) { self.db.publicroomids.insert(room_id, []); }

	/// Removes a room from the public room directory.
	pub fn set_not_public(&self, room_id: &RoomId) { self.db.publicroomids.remove(room_id); }

	/// Lists all public rooms in the directory.
	pub fn public_rooms(&self) -> impl Stream<Item = OwnedRoomId> + Send {
		self.db.publicroomids.keys().ignore_err()
	}

	/// Checks if a room is public (per visibility, not join rule).
	pub async fn is_public_room(&self, room_id: &RoomId) -> bool {
		self.visibility(room_id).await == Visibility::Public
	}

	/// Fetches the visibility of a specific room ID.
	pub async fn visibility(&self, room_id: &RoomId) -> Visibility {
		if self.db.publicroomids.get(room_id).await.is_ok() {
			Visibility::Public
		} else {
			Visibility::Private
		}
	}
}
