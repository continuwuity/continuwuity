use std::{
	collections::{BTreeMap, BTreeSet, HashMap},
	sync::Arc,
};

use conduwuit::{Result, SyncMutex, trace};
use futures::StreamExt;
use ruma::{
	OwnedDeviceId, OwnedRoomId, OwnedUserId, RoomId, UserId, api::client::sync::sync_events::v5,
};
use tokio::sync::{Mutex, watch};

use crate::{Dep, rooms};

pub struct Service {
	services: Services,
	wakers: Wakers,
	snake_connections: DbConnections<SnakeConnectionsKey, SnakeConnectionsVal>,
}

struct Services {
	state_cache: Dep<rooms::state_cache::Service>,
}

#[allow(unused, reason = "TODO refactor")]
struct SlidingSyncCache {
	lists: BTreeMap<String, v5::request::List>,
	subscriptions: BTreeMap<OwnedRoomId, v5::request::RoomSubscription>,
	// For every room, the roomsince number
	known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, u64>>,
	extensions: v5::request::Extensions,
}

#[derive(Default)]
struct SnakeSyncCache {
	lists: BTreeMap<String, v5::request::List>,
	subscriptions: BTreeMap<OwnedRoomId, v5::request::RoomSubscription>,
	known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, u64>>,
	extensions: v5::request::Extensions,
}

type DbConnections<K, V> = SyncMutex<BTreeMap<K, V>>;
type DbConnectionsKey = (OwnedUserId, OwnedDeviceId, String);
pub type SnakeConnectionsKey = (OwnedUserId, OwnedDeviceId, Option<String>);
type SnakeConnectionsVal = Arc<SyncMutex<SnakeSyncCache>>;

#[derive(Default)]
struct Wakers(Mutex<HashMap<OwnedUserId, watch::Sender<u64>>>);

impl Wakers {
	async fn subscribe(&self, user: &UserId) -> watch::Receiver<u64> {
		self.0
			.lock()
			.await
			.entry(user.to_owned())
			.or_insert_with(|| watch::channel(0).0)
			.subscribe()
	}

	async fn wake(&self, user: &UserId) {
		let mut wakers = self.0.lock().await;
		wake_waker(&mut wakers, user);
	}

	async fn wake_all<I>(&self, users: I)
	where
		I: IntoIterator<Item = OwnedUserId> + Send,
	{
		let mut wakers = self.0.lock().await;
		for user in users {
			wake_waker(&mut wakers, &user);
		}
	}
}

/// Bumps the generation observed by anyone long-polling for this user, reaping
/// the entry once its last subscriber is gone.
fn wake_waker(wakers: &mut HashMap<OwnedUserId, watch::Sender<u64>>, user: &UserId) {
	let Some(waker) = wakers.get(user) else {
		return;
	};

	if waker.receiver_count() == 0 {
		wakers.remove(user);
		return;
	}

	waker.send_modify(|generation| *generation = generation.wrapping_add(1));
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
			},
			wakers: Wakers::default(),
			snake_connections: SyncMutex::new(BTreeMap::new()),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	pub async fn subscribe_to_wake(&self, user: &UserId) -> watch::Receiver<u64> {
		self.wakers.subscribe(user).await
	}

	/// Wake the target user's sync loop. Call this when something
	/// that gets included in a legacy sync response changes.
	///
	/// Be careful where you call this function! In particular, don't call
	/// it in any function that's called by `append_pdu`. `append_pdu` will call
	/// it _after_ it's done appending a PDU, and calling it earlier can cause
	/// hard-to-diagnose race conditions.
	pub async fn wake(&self, user: &UserId) {
		trace!(?user, "Waking user's sync loops");

		self.wakers.wake(user).await;
	}

	/// Wake all of our users who are joined to the specified room.
	pub async fn wake_all_joined(&self, room: &RoomId) {
		trace!(?room, "Waking all joined users' sync loops");
		let users: Vec<_> = self
			.services
			.state_cache
			.active_local_users_in_room(room)
			.collect()
			.await;

		self.wakers.wake_all(users).await;
	}

	pub fn snake_connection_cached(&self, key: &SnakeConnectionsKey) -> bool {
		self.snake_connections.lock().contains_key(key)
	}

	pub fn forget_snake_sync_connection(&self, key: &SnakeConnectionsKey) {
		self.snake_connections.lock().remove(key);
	}

	pub fn update_snake_sync_request_with_cache(
		&self,
		snake_key: &SnakeConnectionsKey,
		request: &mut v5::Request,
	) -> BTreeMap<String, BTreeMap<OwnedRoomId, u64>> {
		let mut cache = self.snake_connections.lock();
		let cached = Arc::clone(
			cache
				.entry(snake_key.clone())
				.or_insert_with(|| Arc::new(SyncMutex::new(SnakeSyncCache::default()))),
		);
		let cached = &mut cached.lock();
		drop(cache);

		//v5::Request::try_from_http_request(req, path_args);
		for (list_id, list) in &mut request.lists {
			if let Some(cached_list) = cached.lists.get(list_id) {
				list_or_sticky(
					&mut list.room_details.required_state,
					&cached_list.room_details.required_state,
				);

				match (&mut list.filters, cached_list.filters.clone()) {
					| (Some(filters), Some(cached_filters)) => {
						some_or_sticky(&mut filters.is_invite, cached_filters.is_invite);
						// TODO (morguldir): Find out how a client can unset this, probably need
						// to change into an option inside ruma
						list_or_sticky(
							&mut filters.not_room_types,
							&cached_filters.not_room_types,
						);
					},
					| (_, Some(cached_filters)) => list.filters = Some(cached_filters),
					| (Some(list_filters), _) => list.filters = Some(list_filters.clone()),
					| (..) => {},
				}
			}
			cached.lists.insert(list_id.clone(), list.clone());
		}

		cached
			.subscriptions
			.extend(request.room_subscriptions.clone());
		request
			.room_subscriptions
			.extend(cached.subscriptions.clone());

		request.extensions.e2ee.enabled = request
			.extensions
			.e2ee
			.enabled
			.or(cached.extensions.e2ee.enabled);

		request.extensions.to_device.enabled = request
			.extensions
			.to_device
			.enabled
			.or(cached.extensions.to_device.enabled);

		request.extensions.account_data.enabled = request
			.extensions
			.account_data
			.enabled
			.or(cached.extensions.account_data.enabled);
		request.extensions.account_data.lists = request
			.extensions
			.account_data
			.lists
			.clone()
			.or_else(|| cached.extensions.account_data.lists.clone());
		request.extensions.account_data.rooms = request
			.extensions
			.account_data
			.rooms
			.clone()
			.or_else(|| cached.extensions.account_data.rooms.clone());

		some_or_sticky(&mut request.extensions.typing.enabled, cached.extensions.typing.enabled);
		some_or_sticky(
			&mut request.extensions.typing.rooms,
			cached.extensions.typing.rooms.clone(),
		);
		some_or_sticky(
			&mut request.extensions.typing.lists,
			cached.extensions.typing.lists.clone(),
		);
		some_or_sticky(
			&mut request.extensions.receipts.enabled,
			cached.extensions.receipts.enabled,
		);
		some_or_sticky(
			&mut request.extensions.receipts.rooms,
			cached.extensions.receipts.rooms.clone(),
		);
		some_or_sticky(
			&mut request.extensions.receipts.lists,
			cached.extensions.receipts.lists.clone(),
		);

		cached.extensions = request.extensions.clone();
		cached.known_rooms.clone()
	}

	pub fn update_snake_sync_known_rooms(
		&self,
		key: &SnakeConnectionsKey,
		list_id: String,
		new_cached_rooms: BTreeSet<OwnedRoomId>,
		globalsince: u64,
	) {
		assert!(key.2.is_some(), "Some(conn_id) required for this call");
		let mut cache = self.snake_connections.lock();
		let cached = Arc::clone(
			cache
				.entry(key.clone())
				.or_insert_with(|| Arc::new(SyncMutex::new(SnakeSyncCache::default()))),
		);
		let cached = &mut cached.lock();
		drop(cache);

		for (room_id, lastsince) in cached
			.known_rooms
			.entry(list_id.clone())
			.or_default()
			.iter_mut()
		{
			if !new_cached_rooms.contains(room_id) {
				*lastsince = 0;
			}
		}
		let list = cached.known_rooms.entry(list_id).or_default();
		for room_id in new_cached_rooms {
			list.insert(room_id, globalsince);
		}
	}

	pub fn update_snake_sync_subscriptions(
		&self,
		key: &SnakeConnectionsKey,
		subscriptions: BTreeMap<OwnedRoomId, v5::request::RoomSubscription>,
	) {
		let mut cache = self.snake_connections.lock();
		let cached = Arc::clone(
			cache
				.entry(key.clone())
				.or_insert_with(|| Arc::new(SyncMutex::new(SnakeSyncCache::default()))),
		);
		let cached = &mut cached.lock();
		drop(cache);

		cached.subscriptions = subscriptions;
	}
}

#[inline]
pub fn into_snake_key<U, D, C>(user_id: U, device_id: D, conn_id: C) -> SnakeConnectionsKey
where
	U: Into<OwnedUserId>,
	D: Into<OwnedDeviceId>,
	C: Into<Option<String>>,
{
	(user_id.into(), device_id.into(), conn_id.into())
}

#[inline]
pub fn into_db_key<U, D, C>(user_id: U, device_id: D, conn_id: C) -> DbConnectionsKey
where
	U: Into<OwnedUserId>,
	D: Into<OwnedDeviceId>,
	C: Into<String>,
{
	(user_id.into(), device_id.into(), conn_id.into())
}

/// load params from cache if body doesn't contain it, as long as it's allowed
/// in some cases we may need to allow an empty list as an actual value
fn list_or_sticky<T: Clone>(target: &mut Vec<T>, cached: &Vec<T>) {
	if target.is_empty() {
		target.clone_from(cached);
	}
}

fn some_or_sticky<T>(target: &mut Option<T>, cached: Option<T>) {
	if target.is_none() {
		*target = cached;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn wake_after_subscription_is_observable() {
		let wakers = Wakers::default();
		let user = UserId::parse("@alice:example.com").unwrap();
		let mut receiver = wakers.subscribe(&user).await;
		let generation = *receiver.borrow_and_update();

		wakers.wake(&user).await;
		wakers.wake(&user).await;

		receiver.changed().await.unwrap();
		assert_eq!(*receiver.borrow_and_update(), generation.wrapping_add(2));
	}

	#[tokio::test]
	async fn wake_notifies_all_subscribers() {
		let wakers = Wakers::default();
		let user = UserId::parse("@alice:example.com").unwrap();
		let mut first = wakers.subscribe(&user).await;
		let mut second = wakers.subscribe(&user).await;

		wakers.wake(&user).await;

		first.changed().await.unwrap();
		second.changed().await.unwrap();
	}

	#[tokio::test]
	async fn waker_is_not_retained_without_subscribers() {
		let wakers = Wakers::default();
		let user = UserId::parse("@alice:example.com").unwrap();

		wakers.wake(&user).await;
		assert!(wakers.0.lock().await.is_empty());

		drop(wakers.subscribe(&user).await);
		assert_eq!(wakers.0.lock().await.len(), 1);

		wakers.wake(&user).await;
		assert!(wakers.0.lock().await.is_empty());
	}

	#[tokio::test]
	async fn subscriber_still_woken_after_a_sibling_disconnects() {
		let wakers = Wakers::default();
		let user = UserId::parse("@alice:example.com").unwrap();
		let mut receiver = wakers.subscribe(&user).await;
		drop(wakers.subscribe(&user).await);

		wakers.wake(&user).await;

		receiver.changed().await.unwrap();
	}
}
