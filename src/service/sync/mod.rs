mod watch;

use std::{
	collections::{BTreeMap, BTreeSet},
	sync::Arc,
};

use conduwuit::{Result, Server, SyncMutex};
use database::Map;
use ruma::{
	OwnedDeviceId, OwnedRoomId, OwnedUserId,
	api::client::sync::sync_events::{
		self,
		v4::{ExtensionsConfig, SyncRequestList},
		v5,
	},
};

use crate::{Dep, rooms};

pub struct Service {
	db: Data,
	services: Services,
	connections: DbConnections<DbConnectionsKey, DbConnectionsVal>,
	snake_connections: DbConnections<SnakeConnectionsKey, SnakeConnectionsVal>,
}

pub struct Data {
	todeviceid_events: Arc<Map>,
	userroomid_joined: Arc<Map>,
	userroomid_invitestate: Arc<Map>,
	userroomid_leftstate: Arc<Map>,
	userroomid_notificationcount: Arc<Map>,
	userroomid_highlightcount: Arc<Map>,
	pduid_pdu: Arc<Map>,
	keychangeid_userid: Arc<Map>,
	roomusertype_roomuserdataid: Arc<Map>,
	readreceiptid_readreceipt: Arc<Map>,
	userid_lastonetimekeyupdate: Arc<Map>,
}

struct Services {
	server: Arc<Server>,
	short: Dep<rooms::short::Service>,
	state_cache: Dep<rooms::state_cache::Service>,
	typing: Dep<rooms::typing::Service>,
}

struct SlidingSyncCache {
	lists: BTreeMap<String, SyncRequestList>,
	subscriptions: BTreeMap<OwnedRoomId, sync_events::v4::RoomSubscription>,
	// For every room, the roomsince number
	known_rooms: BTreeMap<String, BTreeMap<OwnedRoomId, u64>>,
	extensions: ExtensionsConfig,
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
type DbConnectionsVal = Arc<SyncMutex<SlidingSyncCache>>;
type SnakeConnectionsKey = (OwnedUserId, OwnedDeviceId, Option<String>);
type SnakeConnectionsVal = Arc<SyncMutex<SnakeSyncCache>>;

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				todeviceid_events: args.db["todeviceid_events"].clone(),
				userroomid_joined: args.db["userroomid_joined"].clone(),
				userroomid_invitestate: args.db["userroomid_invitestate"].clone(),
				userroomid_leftstate: args.db["userroomid_leftstate"].clone(),
				userroomid_notificationcount: args.db["userroomid_notificationcount"].clone(),
				userroomid_highlightcount: args.db["userroomid_highlightcount"].clone(),
				pduid_pdu: args.db["pduid_pdu"].clone(),
				keychangeid_userid: args.db["keychangeid_userid"].clone(),
				roomusertype_roomuserdataid: args.db["roomusertype_roomuserdataid"].clone(),
				readreceiptid_readreceipt: args.db["readreceiptid_readreceipt"].clone(),
				userid_lastonetimekeyupdate: args.db["userid_lastonetimekeyupdate"].clone(),
			},
			services: Services {
				server: args.server.clone(),
				short: args.depend::<rooms::short::Service>("rooms::short"),
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
				typing: args.depend::<rooms::typing::Service>("rooms::typing"),
			},
			connections: SyncMutex::new(BTreeMap::new()),
			snake_connections: SyncMutex::new(BTreeMap::new()),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	pub fn snake_connection_cached(&self, key: &SnakeConnectionsKey) -> bool {
		self.snake_connections.lock().contains_key(key)
	}

	pub fn forget_snake_sync_connection(&self, key: &SnakeConnectionsKey) {
		self.snake_connections.lock().remove(key);
	}

	pub fn remembered(&self, key: &DbConnectionsKey) -> bool {
		self.connections.lock().contains_key(key)
	}

	pub fn forget_sync_request_connection(&self, key: &DbConnectionsKey) {
		self.connections.lock().remove(key);
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
				some_or_sticky(&mut list.include_heroes, cached_list.include_heroes);

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

	pub fn update_sync_request_with_cache(
		&self,
		key: &SnakeConnectionsKey,
		request: &mut sync_events::v4::Request,
	) -> BTreeMap<String, BTreeMap<OwnedRoomId, u64>> {
		let Some(conn_id) = request.conn_id.clone() else {
			return BTreeMap::new();
		};

		let key = into_db_key(key.0.clone(), key.1.clone(), conn_id);
		let mut cache = self.connections.lock();
		let cached = Arc::clone(cache.entry(key).or_insert_with(|| {
			Arc::new(SyncMutex::new(SlidingSyncCache {
				lists: BTreeMap::new(),
				subscriptions: BTreeMap::new(),
				known_rooms: BTreeMap::new(),
				extensions: ExtensionsConfig::default(),
			}))
		}));
		let cached = &mut cached.lock();
		drop(cache);

		for (list_id, list) in &mut request.lists {
			if let Some(cached_list) = cached.lists.get(list_id) {
				list_or_sticky(&mut list.sort, &cached_list.sort);
				list_or_sticky(
					&mut list.room_details.required_state,
					&cached_list.room_details.required_state,
				);
				some_or_sticky(
					&mut list.room_details.timeline_limit,
					cached_list.room_details.timeline_limit,
				);
				some_or_sticky(
					&mut list.include_old_rooms,
					cached_list.include_old_rooms.clone(),
				);
				match (&mut list.filters, cached_list.filters.clone()) {
					| (Some(filter), Some(cached_filter)) => {
						some_or_sticky(&mut filter.is_dm, cached_filter.is_dm);
						list_or_sticky(&mut filter.spaces, &cached_filter.spaces);
						some_or_sticky(&mut filter.is_encrypted, cached_filter.is_encrypted);
						some_or_sticky(&mut filter.is_invite, cached_filter.is_invite);
						list_or_sticky(&mut filter.room_types, &cached_filter.room_types);
						// Should be made possible to change
						list_or_sticky(&mut filter.not_room_types, &cached_filter.not_room_types);
						some_or_sticky(&mut filter.room_name_like, cached_filter.room_name_like);
						list_or_sticky(&mut filter.tags, &cached_filter.tags);
						list_or_sticky(&mut filter.not_tags, &cached_filter.not_tags);
					},
					| (_, Some(cached_filters)) => list.filters = Some(cached_filters),
					| (Some(list_filters), _) => list.filters = Some(list_filters.clone()),
					| (..) => {},
				}
				list_or_sticky(&mut list.bump_event_types, &cached_list.bump_event_types);
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

		cached.extensions = request.extensions.clone();

		cached.known_rooms.clone()
	}

	pub fn update_sync_subscriptions(
		&self,
		key: &DbConnectionsKey,
		subscriptions: BTreeMap<OwnedRoomId, sync_events::v4::RoomSubscription>,
	) {
		let mut cache = self.connections.lock();
		let cached = Arc::clone(cache.entry(key.clone()).or_insert_with(|| {
			Arc::new(SyncMutex::new(SlidingSyncCache {
				lists: BTreeMap::new(),
				subscriptions: BTreeMap::new(),
				known_rooms: BTreeMap::new(),
				extensions: ExtensionsConfig::default(),
			}))
		}));
		let cached = &mut cached.lock();
		drop(cache);

		cached.subscriptions = subscriptions;
	}

	pub fn update_sync_known_rooms(
		&self,
		key: &DbConnectionsKey,
		list_id: String,
		new_cached_rooms: BTreeSet<OwnedRoomId>,
		globalsince: u64,
	) {
		let mut cache = self.connections.lock();
		let cached = Arc::clone(cache.entry(key.clone()).or_insert_with(|| {
			Arc::new(SyncMutex::new(SlidingSyncCache {
				lists: BTreeMap::new(),
				subscriptions: BTreeMap::new(),
				known_rooms: BTreeMap::new(),
				extensions: ExtensionsConfig::default(),
			}))
		}));
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
