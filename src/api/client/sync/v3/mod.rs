mod joined;
mod left;
mod state;

use std::{
	cmp::{self},
	collections::{BTreeMap, HashMap, HashSet},
	time::Duration,
};

use axum::extract::State;
use conduwuit::{
	Result, extract_variant,
	utils::{
		ReadyExt, TryFutureExtExt,
		stream::{BroadbandExt, Tools, WidebandExt},
	},
	warn,
};
use conduwuit_service::Services;
use futures::{
	FutureExt, StreamExt, TryFutureExt,
	future::{OptionFuture, join3, join4, join5},
};
use ruma::{
	DeviceId, OwnedUserId, RoomId, UserId,
	api::client::{
		filter::FilterDefinition,
		sync::sync_events::{
			self, DeviceLists,
			v3::{
				Filter, GlobalAccountData, InviteState, InvitedRoom, KnockState, KnockedRoom,
				Presence, Rooms, ToDevice,
			},
		},
		uiaa::UiaaResponse,
	},
	events::{
		AnyRawAccountDataEvent,
		presence::{PresenceEvent, PresenceEventContent},
	},
	serde::Raw,
};
use service::rooms::lazy_loading::{self, MemberSet, Options as _};

use super::{load_timeline, share_encrypted_room};
use crate::{
	Ruma, RumaResponse,
	client::{
		is_ignored_invite,
		sync::v3::{joined::load_joined_room, left::load_left_room},
	},
};

struct DeviceListUpdates {
	changed: HashSet<OwnedUserId>,
	left: HashSet<OwnedUserId>,
}

impl DeviceListUpdates {
	fn new() -> Self {
		Self {
			changed: HashSet::new(),
			left: HashSet::new(),
		}
	}

	fn merge(&mut self, other: Self) {
		self.changed.extend(other.changed);
		self.left.extend(other.left);
	}
}

impl From<DeviceListUpdates> for DeviceLists {
	fn from(val: DeviceListUpdates) -> Self {
		Self {
			changed: val.changed.into_iter().collect(),
			left: val.left.into_iter().collect(),
		}
	}
}

#[derive(Clone, Copy)]
struct SyncContext<'a> {
	sender_user: &'a UserId,
	sender_device: &'a DeviceId,
	since: Option<u64>,
	next_batch: u64,
	full_state: bool,
	filter: &'a FilterDefinition,
}

impl<'a> SyncContext<'a> {
	fn lazy_loading_context(&self, room_id: &'a RoomId) -> lazy_loading::Context<'a> {
		lazy_loading::Context {
			user_id: self.sender_user,
			device_id: Some(self.sender_device),
			room_id,
			token: self.since,
			options: Some(&self.filter.room.state.lazy_load_options),
		}
	}

	#[inline]
	fn lazy_loading_enabled(&self) -> bool {
		(self.filter.room.state.lazy_load_options.is_enabled()
			|| self.filter.room.timeline.lazy_load_options.is_enabled())
			&& !self.full_state
	}
}

type PresenceUpdates = HashMap<OwnedUserId, PresenceEventContent>;

/// # `GET /_matrix/client/r0/sync`
///
/// Synchronize the client's state with the latest state on the server.
///
/// - This endpoint takes a `since` parameter which should be the `next_batch`
///   value from a previous request for incremental syncs.
///
/// Calling this endpoint without a `since` parameter returns:
/// - Some of the most recent events of each timeline
/// - Notification counts for each room
/// - Joined and invited member counts, heroes
/// - All state events
///
/// Calling this endpoint with a `since` parameter from a previous `next_batch`
/// returns: For joined rooms:
/// - Some of the most recent events of each timeline that happened after since
/// - If user joined the room after since: All state events (unless lazy loading
///   is activated) and all device list updates in that room
/// - If the user was already in the room: A list of all events that are in the
///   state now, but were not in the state at `since`
/// - If the state we send contains a member event: Joined and invited member
///   counts, heroes
/// - Device list updates that happened after `since`
/// - If there are events in the timeline we send or the user send updated his
///   read mark: Notification counts
/// - EDUs that are active now (read receipts, typing updates, presence)
/// - TODO: Allow multiple sync streams to support Pantalaimon
///
/// For invited rooms:
/// - If the user was invited after `since`: A subset of the state of the room
///   at the point of the invite
///
/// For left rooms:
/// - If the user left after `since`: `prev_batch` token, empty state (TODO:
///   subset of the state at the point of the leave)
#[tracing::instrument(
	name = "sync",
	level = "debug",
	skip_all,
	fields(
		since = %body.body.since.as_deref().unwrap_or_default(),
    )
)]
pub(crate) async fn sync_events_route(
	State(services): State<crate::State>,
	body: Ruma<sync_events::v3::Request>,
) -> Result<sync_events::v3::Response, RumaResponse<UiaaResponse>> {
	let (sender_user, sender_device) = body.sender();

	// Presence update
	if services.config.allow_local_presence {
		services
			.presence
			.ping_presence(sender_user, &body.body.set_presence)
			.await?;
	}

	// Setup watchers, so if there's no response, we can wait for them
	let watcher = services.sync.watch(sender_user, sender_device);

	let response = build_sync_events(&services, &body).await?;
	if body.body.full_state
		|| !(response.rooms.is_empty()
			&& response.presence.is_empty()
			&& response.account_data.is_empty()
			&& response.device_lists.is_empty()
			&& response.to_device.is_empty())
	{
		return Ok(response);
	}

	// Hang a few seconds so requests are not spammed
	// Stop hanging if new info arrives
	let default = Duration::from_secs(30);
	let duration = cmp::min(body.body.timeout.unwrap_or(default), default);
	_ = tokio::time::timeout(duration, watcher).await;

	// Retry returning data
	build_sync_events(&services, &body).await
}

pub(crate) async fn build_sync_events(
	services: &Services,
	body: &Ruma<sync_events::v3::Request>,
) -> Result<sync_events::v3::Response, RumaResponse<UiaaResponse>> {
	let (sender_user, sender_device) = body.sender();

	let next_batch = services.globals.current_count()?;
	let since = body
		.body
		.since
		.as_ref()
		.and_then(|string| string.parse().ok());

	let full_state = body.body.full_state;

	// FilterDefinition is very large (0x1000 bytes), let's put it on the heap
	let filter = Box::new(match body.body.filter.as_ref() {
		| None => FilterDefinition::default(),
		| Some(Filter::FilterDefinition(filter)) => filter.clone(),
		| Some(Filter::FilterId(filter_id)) => services
			.users
			.get_filter(sender_user, filter_id)
			.await
			.unwrap_or_default(),
	});

	let context = SyncContext {
		sender_user,
		sender_device,
		since,
		next_batch,
		full_state,
		filter: &filter,
	};

	let joined_rooms = services
		.rooms
		.state_cache
		.rooms_joined(sender_user)
		.map(ToOwned::to_owned)
		.broad_filter_map(|room_id| async {
			let joined_room = load_joined_room(services, context, room_id.clone()).await;

			match joined_room {
				| Ok((room, updates)) => Some((room_id, room, updates)),
				| Err(err) => {
					warn!(?err, ?room_id, "error loading joined room {}", room_id);
					None
				},
			}
		})
		.ready_fold(
			(BTreeMap::new(), DeviceListUpdates::new()),
			|(mut joined_rooms, mut all_updates), (room_id, joined_room, updates)| {
				all_updates.merge(updates);

				if !joined_room.is_empty() {
					joined_rooms.insert(room_id, joined_room);
				}

				(joined_rooms, all_updates)
			},
		);

	let left_rooms = services
		.rooms
		.state_cache
		.rooms_left(sender_user)
		.broad_filter_map(|(room_id, leave_pdu)| {
			load_left_room(services, context, room_id.clone(), leave_pdu)
				.map_ok(move |left_room| (room_id, left_room))
				.ok()
		})
		.ready_filter_map(|(room_id, left_room)| left_room.map(|left_room| (room_id, left_room)))
		.collect();

	let invited_rooms = services
		.rooms
		.state_cache
		.rooms_invited(sender_user)
		.wide_filter_map(async |(room_id, invite_state)| {
			if is_ignored_invite(services, sender_user, &room_id).await {
				None
			} else {
				Some((room_id, invite_state))
			}
		})
		.fold_default(|mut invited_rooms: BTreeMap<_, _>, (room_id, invite_state)| async move {
			let invite_count = services
				.rooms
				.state_cache
				.get_invite_count(&room_id, sender_user)
				.await
				.ok();

			// only sync this invite if it was sent after the last /sync call
			if since < invite_count {
				let invited_room = InvitedRoom {
					invite_state: InviteState { events: invite_state },
				};

				invited_rooms.insert(room_id, invited_room);
			}
			invited_rooms
		});

	let knocked_rooms = services
		.rooms
		.state_cache
		.rooms_knocked(sender_user)
		.fold_default(|mut knocked_rooms: BTreeMap<_, _>, (room_id, knock_state)| async move {
			let knock_count = services
				.rooms
				.state_cache
				.get_knock_count(&room_id, sender_user)
				.await
				.ok();

			// only sync this knock if it was sent after the last /sync call
			if since < knock_count {
				let knocked_room = KnockedRoom {
					knock_state: KnockState { events: knock_state },
				};

				knocked_rooms.insert(room_id, knocked_room);
			}
			knocked_rooms
		});

	let presence_updates: OptionFuture<_> = services
		.config
		.allow_local_presence
		.then(|| process_presence_updates(services, since, sender_user))
		.into();

	let account_data = services
		.account_data
		.changes_since(None, sender_user, since, Some(next_batch))
		.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Global))
		.collect();

	// Look for device list updates of this account
	let keys_changed = services
		.users
		.keys_changed(sender_user, since, Some(next_batch))
		.map(ToOwned::to_owned)
		.collect::<HashSet<_>>();

	let to_device_events = services
		.users
		.get_to_device_events(sender_user, sender_device, since, Some(next_batch))
		.collect::<Vec<_>>();

	let device_one_time_keys_count = services
		.users
		.count_one_time_keys(sender_user, sender_device);

	// Remove all to-device events the device received *last time*
	let remove_to_device_events =
		services
			.users
			.remove_to_device_events(sender_user, sender_device, since);

	let rooms = join4(joined_rooms, left_rooms, invited_rooms, knocked_rooms);
	let ephemeral = join3(remove_to_device_events, to_device_events, presence_updates);
	let top = join5(account_data, ephemeral, device_one_time_keys_count, keys_changed, rooms)
		.boxed()
		.await;

	let (account_data, ephemeral, device_one_time_keys_count, keys_changed, rooms) = top;
	let ((), to_device_events, presence_updates) = ephemeral;
	let (joined_rooms, left_rooms, invited_rooms, knocked_rooms) = rooms;
	let (joined_rooms, mut device_list_updates) = joined_rooms;
	device_list_updates.changed.extend(keys_changed);

	let response = sync_events::v3::Response {
		account_data: GlobalAccountData { events: account_data },
		device_lists: device_list_updates.into(),
		device_one_time_keys_count,
		// Fallback keys are not yet supported
		device_unused_fallback_key_types: None,
		next_batch: next_batch.to_string(),
		presence: Presence {
			events: presence_updates
				.into_iter()
				.flat_map(IntoIterator::into_iter)
				.map(|(sender, content)| PresenceEvent { content, sender })
				.map(|ref event| Raw::new(event))
				.filter_map(Result::ok)
				.collect(),
		},
		rooms: Rooms {
			leave: left_rooms,
			join: joined_rooms,
			invite: invited_rooms,
			knock: knocked_rooms,
		},
		to_device: ToDevice { events: to_device_events },
	};

	Ok(response)
}

#[tracing::instrument(name = "presence", level = "debug", skip_all)]
async fn process_presence_updates(
	services: &Services,
	since: Option<u64>,
	syncing_user: &UserId,
) -> PresenceUpdates {
	services
		.presence
		.presence_since(since.unwrap_or(0)) // send all presences on initial sync
		.filter(|(user_id, ..)| {
			services
				.rooms
				.state_cache
				.user_sees_user(syncing_user, user_id)
		})
		.filter_map(|(user_id, _, presence_bytes)| {
			services
				.presence
				.from_json_bytes_to_event(presence_bytes, user_id)
				.map_ok(move |event| (user_id, event))
				.ok()
		})
		.map(|(user_id, event)| (user_id.to_owned(), event.content))
		.collect()
		.await
}

async fn prepare_lazily_loaded_members(
	services: &Services,
	sync_context: SyncContext<'_>,
	room_id: &RoomId,
	timeline_members: impl Iterator<Item = OwnedUserId>,
) -> Option<MemberSet> {
	let lazy_loading_context = &sync_context.lazy_loading_context(room_id);

	// the user IDs of members whose membership needs to be sent to the client, if
	// lazy-loading is enabled.
	let lazily_loaded_members =
		OptionFuture::from(sync_context.lazy_loading_enabled().then(|| {
			services
				.rooms
				.lazy_loading
				.retain_lazy_members(timeline_members.collect(), lazy_loading_context)
		}))
		.await;

	// reset lazy loading state on initial sync
	if sync_context.since.is_none() {
		services
			.rooms
			.lazy_loading
			.reset(lazy_loading_context)
			.await;
	}

	lazily_loaded_members
}
