use std::{
	cmp::{self, Ordering},
	collections::{BTreeMap, BTreeSet, HashMap, HashSet},
	time::Duration,
};

use axum::extract::State;
use conduwuit::{
	Err, Error, Result, at, error, extract_variant, is_equal_to,
	matrix::{
		Event, TypeStateKey,
		pdu::{PduCount, PduEvent, sticky},
	},
	trace,
	utils::{
		BoolExt, FutureBoolExt, IterStream, ReadyExt, TryFutureExtExt,
		future::ReadyEqExt,
		math::{ruma_from_usize, usize_from_ruma},
		stream::{TryIgnore, WidebandExt},
	},
	warn,
};
use conduwuit_service::{
	Services,
	rooms::read_receipt::pack_receipts,
	sync::{SnakeConnectionsKey, into_snake_key},
};
use futures::{
	FutureExt, StreamExt, TryFutureExt,
	future::{OptionFuture, join3, try_join4},
	pin_mut,
};
use ruma::{
	DeviceId, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, RoomId, UInt, UserId,
	api::client::sync::sync_events::{
		self, DeviceLists, UnreadNotificationsCount, v5::request::ExtensionRoomConfig,
	},
	assign,
	directory::RoomTypeFilter,
	events::{
		AnyStrippedStateEvent, AnySyncEphemeralRoomEvent, AnySyncStateEvent,
		GlobalAccountDataEventType, StateEventType, TimelineEventType,
		direct::DirectEvent,
		room::{
			create::RoomCreateEventContent,
			member::{MembershipState, RoomMemberEventContent},
		},
		typing::{SyncTypingEvent, TypingEventContent},
	},
	serde::Raw,
	uint,
};
use service::account_data::AnyRawAccountDataEvent;
use tokio::pin;

use super::share_encrypted_room;
use crate::{
	Ruma,
	client::{
		DEFAULT_BUMP_TYPES, TimelinePdus, ignored_filter, is_ignored_invite, sync::load_timeline,
	},
	client_ip::ClientIp,
};

type SyncInfo<'a> = (&'a UserId, &'a DeviceId, u64, &'a sync_events::v5::Request);
type TodoRooms = BTreeMap<OwnedRoomId, (BTreeSet<TypeStateKey>, usize, u64)>;
type KnownRooms = BTreeMap<String, BTreeMap<OwnedRoomId, u64>>;
type KnownRoomUpdates = BTreeMap<String, BTreeSet<OwnedRoomId>>;

/// Default and maximum number of sticky events returned per response.
const DEFAULT_STICKY_LIMIT: usize = 100;

fn num_live_events<'a>(
	counts: impl DoubleEndedIterator<Item = &'a PduCount>,
	globalsince: u64,
) -> usize {
	if globalsince == 0 {
		return 0;
	}

	counts
		.rev()
		.take_while(|count| matches!(count, PduCount::Normal(count) if *count > globalsince))
		.count()
}

struct SyncCollection {
	response: sync_events::v5::Response,
	known_room_updates: KnownRoomUpdates,
}

/// `POST /_matrix/client/unstable/org.matrix.simplified_msc3575/sync`
/// ([MSC4186])
///
/// A simplified version of sliding sync ([MSC3575]).
///
/// Get all new events in a sliding window of rooms since the last sync or a
/// given point in time.
///
/// [MSC3575]: https://github.com/matrix-org/matrix-spec-proposals/pull/3575
/// [MSC4186]: https://github.com/matrix-org/matrix-spec-proposals/pull/4186
pub(crate) async fn sync_events_v5_route(
	State(ref services): State<crate::State>,
	ClientIp(client_ip): ClientIp, // NOTE: Required for updating device metadata
	body: Ruma<sync_events::v5::Request>,
) -> Result<sync_events::v5::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	let sender_device = body.identity.expect_sender_device()?;

	services
		.users
		.update_device_last_seen(sender_user, Some(sender_device), client_ip)
		.await;

	let mut body = body.body;

	let conn_id = body.conn_id.clone();

	let globalsince = body
		.pos
		.as_ref()
		.and_then(|string| string.parse().ok())
		.unwrap_or(0);

	let snake_key = into_snake_key(sender_user, sender_device.as_str(), conn_id);

	if globalsince != 0 && !services.sync.snake_connection_cached(&snake_key) {
		return Err!(Request(UnknownPos(
			"Connection data unknown to server; restarting sync stream."
		)));
	}

	// Client / User requested an initial sync
	if globalsince == 0 {
		services.sync.forget_snake_sync_connection(&snake_key);
	}

	// Get sticky parameters from cache
	let known_rooms = services
		.sync
		.update_snake_sync_request_with_cache(&snake_key, &mut body);

	let mut wake_receiver = services.sync.subscribe_to_wake(sender_user).await;
	let mut collection = collect_sync_response(
		services,
		sender_user,
		sender_device,
		&body,
		globalsince,
		&known_rooms,
	)
	.await?;

	if response_is_empty(&collection.response) {
		let default = Duration::from_secs(30);
		let duration = cmp::min(body.timeout.unwrap_or(default), default);
		let woke = tokio::time::timeout(duration, wake_receiver.changed())
			.await
			.is_ok();

		if woke {
			collection = collect_sync_response(
				services,
				sender_user,
				sender_device,
				&body,
				globalsince,
				&known_rooms,
			)
			.await?;
		}
	}

	commit_sync_collection(
		services,
		sender_user,
		sender_device,
		&body,
		globalsince,
		&snake_key,
		&collection.known_room_updates,
	)
	.await;

	trace!(
		rooms = ?collection.response.rooms.len(),
		account_data = ?collection.response.extensions.account_data.rooms.len(),
		receipts = ?collection.response.extensions.receipts.rooms.len(),
		"responding to request with"
	);
	Ok(collection.response)
}

async fn collect_sync_response(
	services: &Services,
	sender_user: &UserId,
	sender_device: &DeviceId,
	body: &sync_events::v5::Request,
	globalsince: u64,
	known_rooms: &KnownRooms,
) -> Result<SyncCollection> {
	// Bounds every read below and becomes the `pos` returned to the client. Zero
	// would be echoed back as a request for an initial sync.
	let next_batch = services.globals.current_count()?.max(1);

	let all_joined_rooms = services
		.rooms
		.state_cache
		.rooms_joined(sender_user)
		.collect::<Vec<OwnedRoomId>>();

	let all_invited_rooms = services
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
		.map(|r| r.0)
		.collect::<Vec<OwnedRoomId>>();

	let all_knocked_rooms = services
		.rooms
		.state_cache
		.rooms_knocked(sender_user)
		.map(|r| r.0)
		.collect::<Vec<OwnedRoomId>>();

	let (all_joined_rooms, all_invited_rooms, all_knocked_rooms) =
		join3(all_joined_rooms, all_invited_rooms, all_knocked_rooms).await;

	let allowed_rooms: BTreeSet<OwnedRoomId> = all_joined_rooms
		.iter()
		.chain(all_invited_rooms.iter())
		.chain(all_knocked_rooms.iter())
		.cloned()
		.collect();

	let all_joined_rooms = all_joined_rooms.iter().map(AsRef::as_ref);
	let joined_room_ids = all_joined_rooms
		.clone()
		.map(ToOwned::to_owned)
		.collect::<BTreeSet<_>>();
	let all_invited_rooms = all_invited_rooms.iter().map(AsRef::as_ref);
	let all_knocked_rooms = all_knocked_rooms.iter().map(AsRef::as_ref);
	let all_rooms = all_joined_rooms
		.clone()
		.chain(all_invited_rooms.clone())
		.chain(all_knocked_rooms.clone());

	let pos = next_batch.to_string();

	let mut todo_rooms: TodoRooms = BTreeMap::new();
	let mut sticky_rooms = BTreeSet::new();

	let sync_info: SyncInfo<'_> = (sender_user, sender_device, globalsince, body);

	let account_data = collect_account_data(services, sync_info).map(Ok);

	let e2ee = collect_e2ee(services, sync_info, all_joined_rooms.clone());

	let to_device = collect_to_device(services, sync_info, next_batch).map(Ok);

	let receipts = collect_receipts(services).map(Ok);

	let (account_data, e2ee, to_device, receipts) =
		try_join4(account_data, e2ee, to_device, receipts).await?;

	let extensions = assign!(sync_events::v5::response::Extensions::default(), {
		account_data,
		e2ee,
		to_device,
		receipts,
		typing: sync_events::v5::response::Typing::default(),
		sticky_events: sync_events::v5::response::StickyEvents::default(),
	});

	let mut response = assign!(sync_events::v5::Response::new(pos), {
		txn_id: body.txn_id.clone(),
		lists: BTreeMap::new(),
		rooms: BTreeMap::new(),
		extensions,
	});
	let direct_rooms = if body.lists.values().any(|list| {
		list.filters
			.as_ref()
			.is_some_and(|filters| filters.is_dm.is_some())
	}) {
		match services
			.account_data
			.get_global::<DirectEvent>(sender_user, GlobalAccountDataEventType::Direct)
			.await
		{
			| Ok(event) => event.content.0.into_values().flatten().collect(),
			| Err(error) if error.is_not_found() => HashSet::new(),
			| Err(error) => return Err(error),
		}
	} else {
		HashSet::new()
	};

	let mut known_room_updates = handle_lists(
		services,
		sender_user,
		body,
		all_invited_rooms.clone(),
		all_joined_rooms.clone(),
		all_rooms,
		&direct_rooms,
		&mut todo_rooms,
		&mut sticky_rooms,
		known_rooms,
		&mut response,
	)
	.await;

	known_room_updates.insert(
		"subscriptions".to_owned(),
		fetch_subscriptions(services, body, known_rooms, &allowed_rooms, &mut todo_rooms).await,
	);
	sticky_rooms.extend(body.room_subscriptions.keys().cloned());

	let mut timeline_event_ids = BTreeSet::new();
	response.rooms = process_rooms(
		services,
		sender_user,
		globalsince,
		next_batch,
		all_invited_rooms.clone(),
		all_knocked_rooms.clone(),
		&todo_rooms,
		&mut response,
		body,
		&mut timeline_event_ids,
	)
	.await?;

	response.extensions.sticky_events = collect_sticky_events(
		services,
		sender_user,
		body,
		&sticky_rooms,
		&joined_room_ids,
		&todo_rooms,
		&timeline_event_ids,
	)
	.await?;

	response.extensions.typing =
		collect_typing_events(services, sender_user, body, &todo_rooms, &known_room_updates)
			.await?;

	Ok(SyncCollection { response, known_room_updates })
}

fn response_is_empty(response: &sync_events::v5::Response) -> bool {
	let no_account_data = response.extensions.account_data.global.is_empty()
		&& response
			.extensions
			.account_data
			.rooms
			.values()
			.all(Vec::is_empty);
	let no_room_data = response.rooms.iter().all(|(id, room)| {
		room.timeline.is_empty()
			&& room.required_state.is_empty()
			&& room.invite_state.is_none()
			&& !response.extensions.receipts.rooms.contains_key(id)
	});
	let no_to_device_messages = response
		.extensions
		.to_device
		.as_ref()
		.is_none_or(|to| to.events.is_empty());

	no_account_data
		&& no_room_data
		&& no_to_device_messages
		&& response.extensions.sticky_events.is_empty()
		&& response.extensions.typing.is_empty()
}

async fn commit_sync_collection(
	services: &Services,
	sender_user: &UserId,
	sender_device: &DeviceId,
	body: &sync_events::v5::Request,
	globalsince: u64,
	snake_key: &SnakeConnectionsKey,
	known_room_updates: &KnownRoomUpdates,
) {
	let (.., conn_id) = snake_key;

	if conn_id.is_some() {
		for (list_id, rooms) in known_room_updates {
			services.sync.update_snake_sync_known_rooms(
				snake_key,
				list_id.clone(),
				rooms.clone(),
				globalsince,
			);
		}
	}

	if body.extensions.to_device.enabled.unwrap_or(false) {
		services
			.users
			.remove_to_device_events(sender_user, sender_device, globalsince)
			.await;
	}
}

async fn fetch_subscriptions(
	services: &Services,
	body: &sync_events::v5::Request,
	known_rooms: &KnownRooms,
	allowed_rooms: &BTreeSet<OwnedRoomId>,
	todo_rooms: &mut TodoRooms,
) -> BTreeSet<OwnedRoomId> {
	let mut known_subscription_rooms = BTreeSet::new();
	for (room_id, room) in &body.room_subscriptions {
		// Silently ignore subscriptions to rooms the user is not a member of
		// (joined or invited).
		if !allowed_rooms.contains(room_id) {
			continue;
		}

		let not_exists = services.rooms.metadata.exists(room_id).eq(&false);

		let is_disabled = services.rooms.metadata.is_disabled(room_id);

		let is_banned = services.rooms.metadata.is_banned(room_id);

		pin_mut!(not_exists, is_disabled, is_banned);
		if not_exists.or(is_disabled).or(is_banned).await {
			continue;
		}

		let todo_room =
			todo_rooms
				.entry(room_id.clone())
				.or_insert((BTreeSet::new(), 0_usize, u64::MAX));

		let limit: UInt = room.timeline_limit;

		todo_room.0.extend(
			room.required_state
				.iter()
				.map(|(ty, sk)| (ty.clone(), sk.as_str().into())),
		);
		todo_room.1 = todo_room.1.max(usize_from_ruma(limit));
		// 0 means unknown because it got out of date
		todo_room.2 = todo_room.2.min(
			known_rooms
				.get("subscriptions")
				.and_then(|k| k.get(room_id))
				.copied()
				.unwrap_or(0),
		);
		known_subscription_rooms.insert(room_id.clone());
	}
	// where this went (protomsc says it was removed)
	//for r in body.unsubscribe_rooms {
	//	known_subscription_rooms.remove(&r);
	//	body.room_subscriptions.remove(&r);
	//}

	known_subscription_rooms
}

#[allow(clippy::too_many_arguments)]
async fn handle_lists<'a, Rooms, AllRooms>(
	services: &Services,
	sender_user: &UserId,
	body: &sync_events::v5::Request,
	all_invited_rooms: Rooms,
	all_joined_rooms: Rooms,
	all_rooms: AllRooms,
	direct_rooms: &HashSet<OwnedRoomId>,
	todo_rooms: &'a mut TodoRooms,
	sticky_rooms: &mut BTreeSet<OwnedRoomId>,
	known_rooms: &'a KnownRooms,
	response: &'_ mut sync_events::v5::Response,
) -> KnownRoomUpdates
where
	Rooms: Iterator<Item = &'a RoomId> + Clone + Send + 'a,
	AllRooms: Iterator<Item = &'a RoomId> + Clone + Send + 'a,
{
	// TODO MSC4186: ruma's `ListFilters` does not model `spaces`, `tags` or
	// `not_tags`, nor the rename of `is_invite` to `is_invited`.
	let invited_rooms: HashSet<&RoomId> = all_invited_rooms.clone().collect();

	// Memoised for the whole request; lists commonly overlap.
	let mut encrypted_rooms: HashMap<&RoomId, bool> = HashMap::new();
	let mut room_types: HashMap<&RoomId, Option<RoomTypeFilter>> = HashMap::new();

	let mut known_room_updates = KnownRoomUpdates::new();
	for (list_id, list) in &body.lists {
		let filters = list.filters.as_ref();
		let is_dm = filters.and_then(|filters| filters.is_dm);
		let is_encrypted = filters.and_then(|filters| filters.is_encrypted);
		let room_types_filter = filters.map_or(&[][..], |filters| filters.room_types.as_slice());
		let not_room_types = filters.map_or(&[][..], |filters| filters.not_room_types.as_slice());
		let filter_room_types = !room_types_filter.is_empty() || !not_room_types.is_empty();

		let candidate_rooms: Vec<&RoomId> = match filters.and_then(|filters| filters.is_invite) {
			| None => all_rooms.clone().collect(),
			| Some(true) => all_invited_rooms.clone().collect(),
			| Some(false) => all_joined_rooms.clone().collect(),
		};

		let mut active_rooms: Vec<&RoomId> = Vec::with_capacity(candidate_rooms.len());
		for room_id in candidate_rooms {
			if !matches_bool_filter(direct_rooms.contains(room_id), is_dm) {
				continue;
			}

			let invited = invited_rooms.contains(room_id);

			if is_encrypted.is_some() {
				let encrypted = match encrypted_rooms.get(room_id) {
					| Some(encrypted) => *encrypted,
					| None => {
						let encrypted =
							room_is_encrypted(services, sender_user, room_id, invited).await;
						encrypted_rooms.insert(room_id, encrypted);
						encrypted
					},
				};

				if !matches_bool_filter(encrypted, is_encrypted) {
					continue;
				}
			}

			if filter_room_types {
				let room_type = match room_types.get(room_id) {
					| Some(room_type) => room_type.clone(),
					| None => {
						let room_type =
							room_type_filter(services, sender_user, room_id, invited).await;
						room_types.insert(room_id, room_type.clone());
						room_type
					},
				};

				let Some(room_type) = room_type else {
					continue;
				};

				if !matches_room_type(&room_type, room_types_filter, not_room_types) {
					continue;
				}
			}

			active_rooms.push(room_id);
		}
		sticky_rooms.extend(active_rooms.iter().map(|room_id| (*room_id).to_owned()));

		let mut new_known_rooms: BTreeSet<OwnedRoomId> = BTreeSet::new();

		let ranges = list.ranges.clone();

		for mut range in ranges {
			range.0 = range
				.0
				.min(UInt::try_from(active_rooms.len()).unwrap_or(UInt::MAX));
			range.1 = range.1.checked_add(uint!(1)).unwrap_or(range.1);
			range.1 = range
				.1
				.clamp(range.0, UInt::try_from(active_rooms.len()).unwrap_or(UInt::MAX));

			let room_ids =
				active_rooms[usize_from_ruma(range.0)..usize_from_ruma(range.1)].to_vec();

			let new_rooms: BTreeSet<OwnedRoomId> =
				room_ids.clone().into_iter().map(From::from).collect();

			new_known_rooms.extend(new_rooms);
			//new_known_rooms.extend(room_ids..cloned());
			for room_id in room_ids {
				let todo_room = todo_rooms.entry(room_id.to_owned()).or_insert((
					BTreeSet::new(),
					0_usize,
					u64::MAX,
				));

				let limit: usize = usize_from_ruma(list.room_details.timeline_limit).min(100);

				todo_room.0.extend(
					list.room_details
						.required_state
						.iter()
						.map(|(ty, sk)| (ty.clone(), sk.as_str().into())),
				);

				todo_room.1 = todo_room.1.max(limit);
				// 0 means unknown because it got out of date
				todo_room.2 = todo_room.2.min(
					known_rooms
						.get(list_id.as_str())
						.and_then(|k| k.get(room_id))
						.copied()
						.unwrap_or(0),
				);
			}
		}
		response.lists.insert(
			list_id.clone(),
			assign!(sync_events::v5::response::List::default(), {
				count: ruma_from_usize(active_rooms.len()),
			}),
		);

		known_room_updates.insert(list_id.clone(), new_known_rooms);
	}

	known_room_updates
}

fn matches_bool_filter(value: bool, filter: Option<bool>) -> bool {
	filter.is_none_or(|expected| value == expected)
}

/// Rooms we are only invited to have no state locally, so their stripped
/// invite state answers for them instead.
async fn room_is_encrypted(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	invited: bool,
) -> bool {
	if !invited {
		return services
			.rooms
			.state_accessor
			.is_encrypted_room(room_id)
			.await;
	}

	stripped_state_event(services, sender_user, room_id, &StateEventType::RoomEncryption)
		.await
		.is_some()
}

/// Returns `None` if the type could not be determined, in which case the room
/// is excluded from the list.
///
/// Stripped invite state is only recommended to carry `m.room.create`, so an
/// invite without one is treated as untyped rather than hidden from every list.
async fn room_type_filter(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	invited: bool,
) -> Option<RoomTypeFilter> {
	if invited {
		let content: Option<RoomCreateEventContent> =
			stripped_state_event(services, sender_user, room_id, &StateEventType::RoomCreate)
				.await
				.and_then(|event| event.get_field("content").ok().flatten());

		return Some(RoomTypeFilter::from(content.and_then(|content| content.room_type)));
	}

	match services.rooms.state_accessor.get_room_type(room_id).await {
		| Ok(room_type) => Some(RoomTypeFilter::from(Some(room_type))),
		| Err(error) if error.is_not_found() => Some(RoomTypeFilter::Default),
		| Err(error) => {
			warn!(%room_id, %error, "Failed to fetch room type for a sliding sync list filter");
			None
		},
	}
}

async fn stripped_state_event(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	event_type: &StateEventType,
) -> Option<Raw<AnyStrippedStateEvent>> {
	services
		.rooms
		.state_cache
		.invite_state(sender_user, room_id)
		.await
		.ok()?
		.into_iter()
		.find(|event| {
			event
				.get_field::<StateEventType>("type")
				.ok()
				.flatten()
				.as_ref() == Some(event_type)
		})
}

fn matches_room_type(
	room_type: &RoomTypeFilter,
	room_types: &[RoomTypeFilter],
	not_room_types: &[RoomTypeFilter],
) -> bool {
	!not_room_types.contains(room_type)
		&& (room_types.is_empty() || room_types.contains(room_type))
}

#[allow(clippy::too_many_arguments)]
async fn process_rooms<'a, Rooms>(
	services: &Services,
	sender_user: &UserId,
	globalsince: u64,
	next_batch: u64,
	all_invited_rooms: Rooms,
	all_knocked_rooms: Rooms,
	todo_rooms: &TodoRooms,
	response: &mut sync_events::v5::Response,
	body: &sync_events::v5::Request,
	timeline_event_ids: &mut BTreeSet<OwnedEventId>,
) -> Result<BTreeMap<OwnedRoomId, sync_events::v5::response::Room>>
where
	Rooms: Iterator<Item = &'a RoomId> + Clone + Send + 'a,
{
	let mut rooms = BTreeMap::new();
	for (room_id, (required_state_request, timeline_limit, roomsince)) in todo_rooms {
		let roomsincecount = PduCount::Normal(*roomsince);

		let mut timestamp: Option<_> = None;
		let (timeline_pdus, limited);
		let new_room_id: &RoomId = (*room_id).as_ref();
		if all_invited_rooms.clone().any(is_equal_to!(new_room_id)) {
			let Ok(invite_count) = services
				.rooms
				.state_cache
				.get_invite_count(room_id, sender_user)
				.await
			else {
				continue;
			};

			if *roomsince >= invite_count {
				continue;
			}

			// TODO: figure out a timestamp we can use for remote invites
			let invite_state = services
				.rooms
				.state_cache
				.invite_state(sender_user, room_id)
				.await
				.ok();

			rooms.insert(
				room_id.clone(),
				assign!(sync_events::v5::response::Room::new(), {
					initial: Some(roomsince == &0),
					invite_state,
					limited: true,
				}),
			);
			continue;
		}

		if all_knocked_rooms.clone().any(is_equal_to!(new_room_id)) {
			let Ok(knock_count) = services
				.rooms
				.state_cache
				.get_knock_count(room_id, sender_user)
				.await
			else {
				continue;
			};

			if *roomsince >= knock_count {
				continue;
			}

			let Ok(knock_state) = services
				.rooms
				.state_cache
				.knock_state(sender_user, room_id)
				.await
			else {
				continue;
			};

			rooms.insert(
				room_id.clone(),
				assign!(sync_events::v5::response::Room::new(), {
					initial: Some(roomsince == &0),
					invite_state: Some(knock_state),
					limited: true,
				}),
			);
			continue;
		}

		if !services
			.rooms
			.state_cache
			.is_joined(sender_user, room_id)
			.await
		{
			continue;
		}

		TimelinePdus { pdus: timeline_pdus, limited } = match load_timeline(
			services,
			sender_user,
			room_id,
			Some(roomsincecount),
			Some(PduCount::from(next_batch)),
			*timeline_limit,
		)
		.await
		{
			| Ok(value) => value,
			| Err(err) => {
				warn!("Encountered missing timeline in {}, error {}", room_id, err);
				continue;
			},
		};

		if body.extensions.account_data.enabled == Some(true) {
			response.extensions.account_data.rooms.insert(
				room_id.to_owned(),
				services
					.account_data
					.changes_since(Some(room_id), sender_user, Some(*roomsince), Some(next_batch))
					.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
					.collect()
					.await,
			);
		}

		let last_privateread_update = services
			.rooms
			.read_receipt
			.last_privateread_update(sender_user, room_id)
			.await;

		let private_read_event: OptionFuture<_> = (last_privateread_update > *roomsince)
			.then(|| {
				services
					.rooms
					.read_receipt
					.private_read_get(room_id, sender_user)
					.ok()
			})
			.into();

		let mut receipts: Vec<Raw<AnySyncEphemeralRoomEvent>> = services
			.rooms
			.read_receipt
			.readreceipts_since(room_id, Some(*roomsince))
			.filter_map(|(read_user, _ts, v)| async move {
				services
					.users
					.user_is_ignored(&read_user, sender_user)
					.await
					.or_some(v)
			})
			.collect()
			.await;

		if let Some(private_read_event) = private_read_event.await.flatten() {
			receipts.push(private_read_event);
		}

		let receipt_size = receipts.len();

		if receipt_size > 0 {
			response
				.extensions
				.receipts
				.rooms
				.insert(room_id.clone(), pack_receipts(Box::new(receipts.into_iter())));
		}

		if roomsince != &0
			&& timeline_pdus.is_empty()
			&& response
				.extensions
				.account_data
				.rooms
				.get(room_id)
				.is_none_or(Vec::is_empty)
			&& receipt_size == 0
		{
			continue;
		}

		let prev_batch = timeline_pdus
			.front()
			.map_or(Ok::<_, Error>(None), |(pdu_count, _)| {
				Ok(Some(match pdu_count {
					| PduCount::Backfilled(_) => {
						error!("timeline in backfill state?!");
						"0".to_owned()
					},
					| PduCount::Normal(c) => c.to_string(),
				}))
			})?
			.or_else(|| {
				if roomsince != &0 {
					Some(roomsince.to_string())
				} else {
					None
				}
			});

		let required_state =
			collect_required_state(services, room_id, required_state_request).await;

		let timeline_pdus: Vec<_> = timeline_pdus
			.iter()
			.stream()
			.filter_map(|item| ignored_filter(services, item.clone(), sender_user))
			.collect()
			.await;
		let num_live = num_live_events(timeline_pdus.iter().map(|(count, _)| count), globalsince);
		timeline_event_ids.extend(timeline_pdus.iter().map(|(_, pdu)| pdu.event_id.clone()));

		for (_, pdu) in &timeline_pdus {
			let ts = pdu.origin_server_ts;
			if DEFAULT_BUMP_TYPES.binary_search(&pdu.kind).is_ok()
				&& timestamp.is_none_or(|time| time <= ts)
			{
				timestamp = Some(ts);
			}
		}
		let room_events = timeline_pdus
			.into_iter()
			.map(at!(1))
			.map(Event::into_format)
			.collect();

		// Heroes
		let heroes: Vec<_> = services
			.rooms
			.state_cache
			.room_members(room_id)
			.ready_filter(|member| *member != sender_user)
			.filter_map(async |user_id| {
				services
					.rooms
					.state_accessor
					.get_member(room_id, &user_id)
					.map_ok(|member_event| {
						assign!(sync_events::v5::response::Hero::new(user_id.clone()), {
							name: member_event.displayname,
							avatar: member_event.avatar_url,
						})
					})
					.ok()
					.await
			})
			.take(5)
			.collect()
			.await;

		let name = match heroes.len().cmp(&(1_usize)) {
			| Ordering::Greater => {
				let firsts = heroes[1..]
					.iter()
					.map(|h| h.name.clone().unwrap_or_else(|| h.user_id.to_string()))
					.collect::<Vec<_>>()
					.join(", ");

				let last = heroes[0]
					.name
					.clone()
					.unwrap_or_else(|| heroes[0].user_id.to_string());

				Some(format!("{firsts} and {last}"))
			},
			| Ordering::Equal => Some(
				heroes[0]
					.name
					.clone()
					.unwrap_or_else(|| heroes[0].user_id.to_string()),
			),
			| Ordering::Less => None,
		};

		let heroes_avatar = if heroes.len() == 1 {
			heroes[0].avatar.clone()
		} else {
			None
		};

		rooms.insert(
			room_id.clone(),
			assign!(sync_events::v5::response::Room::new(), {
				name: services
					.rooms
					.state_accessor
					.get_name(room_id)
					.await
					.ok()
					.or(name),
				avatar: match heroes_avatar {
					| Some(heroes_avatar) => ruma::JsOption::Some(heroes_avatar),
					| _ => match services.rooms.state_accessor.get_avatar(room_id).await {
						| ruma::JsOption::Some(avatar) => ruma::JsOption::from_option(avatar.url),
						| ruma::JsOption::Null => ruma::JsOption::Null,
						| ruma::JsOption::Undefined => ruma::JsOption::Undefined,
					},
				},
				initial: Some(roomsince == &0),
				is_dm: None,
				unread_notifications: assign!(UnreadNotificationsCount::new(), {
					highlight_count: Some(
						services
							.rooms
							.user
							.highlight_count(sender_user, room_id)
							.await
							.try_into()
							.expect("notification count can't go that high"),
					),
					notification_count: Some(
						services
							.rooms
							.user
							.notification_count(sender_user, room_id)
							.await
							.try_into()
							.expect("notification count can't go that high"),
					),
				}),
				timeline: room_events,
				required_state,
				prev_batch,
				limited,
				joined_count: Some(
					services
						.rooms
						.state_cache
						.room_joined_count(room_id)
						.await
						.unwrap_or(0)
						.try_into()
						.unwrap_or_else(|_| uint!(0)),
				),
				invited_count: Some(
					services
						.rooms
						.state_cache
						.room_invited_count(room_id)
						.await
						.unwrap_or(0)
						.try_into()
						.unwrap_or_else(|_| uint!(0)),
				),
				num_live: Some(ruma_from_usize(num_live)),
				bump_stamp: timestamp,
				heroes: Some(heroes),
			}),
		);
	}
	Ok(rooms)
}

async fn collect_sticky_events(
	services: &Services,
	sender_user: &UserId,
	body: &sync_events::v5::Request,
	sticky_rooms: &BTreeSet<OwnedRoomId>,
	joined_room_ids: &BTreeSet<OwnedRoomId>,
	todo_rooms: &TodoRooms,
	timeline_event_ids: &BTreeSet<OwnedEventId>,
) -> Result<sync_events::v5::response::StickyEvents> {
	if !services.config.allow_sticky_events
		|| !body.extensions.sticky_events.enabled.unwrap_or(false)
	{
		return Ok(sync_events::v5::response::StickyEvents::default());
	}

	let since = body
		.extensions
		.sticky_events
		.since
		.as_deref()
		.and_then(|token| token.parse::<u64>().ok())
		.unwrap_or(0);
	let limit = body
		.extensions
		.sticky_events
		.limit
		.map_or(DEFAULT_STICKY_LIMIT, usize_from_ruma)
		.clamp(1, DEFAULT_STICKY_LIMIT);
	let now = u64::from(MilliSecondsSinceUnixEpoch::now().get());
	let oldest_sticky_ts = now.saturating_sub(sticky::MAX_DURATION_MS);

	let mut backlog = Vec::new();
	let mut stream = Vec::new();
	for room_id in sticky_rooms
		.iter()
		.filter(|room_id| joined_room_ids.contains(*room_id))
	{
		// a room the client has not seen before needs its whole backlog, which a
		// single stream position cannot express
		let initial = todo_rooms
			.get(room_id)
			.is_some_and(|&(_, _, roomsince)| roomsince == 0);

		let pdus = services.rooms.timeline.pdus_rev(room_id, None).ignore_err();
		pin_mut!(pdus);
		while let Some((count, pdu)) = pdus.next().await {
			// nothing older than this can still be sticky, and on the stream we
			// have already delivered everything up to `since`
			if u64::from(pdu.origin_server_ts) < oldest_sticky_ts
				|| (!initial && count <= PduCount::Normal(since))
			{
				break;
			}

			let PduCount::Normal(count) = count else {
				continue;
			};

			let is_sticky = pdu
				.sticky
				.as_deref()
				.is_some_and(|sticky| sticky::is_sticky(pdu.origin_server_ts, sticky, now));

			if !is_sticky || timeline_event_ids.contains(&pdu.event_id) {
				continue;
			}
			if services
				.users
				.user_is_ignored(pdu.sender(), sender_user)
				.await
			{
				continue;
			}

			if count > since {
				stream.push((count, room_id.clone(), pdu));
			} else {
				backlog.push((room_id.clone(), pdu));
			}
		}
	}
	stream.sort_by_key(|(count, ..)| *count);

	let mut response = sync_events::v5::response::StickyEvents::default();

	// MSC4480 lets us ignore the limit for a newly visible room's backlog
	for (room_id, pdu) in backlog {
		push_sticky_event(&mut response, sender_user, room_id, pdu);
	}

	let mut next_batch = None;
	for (count, room_id, pdu) in stream.into_iter().take(limit) {
		push_sticky_event(&mut response, sender_user, room_id, pdu);
		next_batch = Some(count);
	}

	if !response.rooms.is_empty() {
		response.next_batch = Some(next_batch.unwrap_or(since).to_string());
	}

	Ok(response)
}

fn push_sticky_event(
	response: &mut sync_events::v5::response::StickyEvents,
	sender_user: &UserId,
	room_id: OwnedRoomId,
	mut pdu: PduEvent,
) {
	pdu.set_unsigned(Some(sender_user));
	response
		.rooms
		.entry(room_id)
		.or_default()
		.events
		.push(Event::into_format(pdu));
}

/// Collect the required state events for a room
async fn collect_required_state(
	services: &Services,
	room_id: &RoomId,
	required_state_request: &BTreeSet<TypeStateKey>,
) -> Vec<Raw<AnySyncStateEvent>> {
	let mut required_state = Vec::new();
	let mut wildcard_types: HashSet<&StateEventType> = HashSet::new();

	for (event_type, state_key) in required_state_request {
		if wildcard_types.contains(event_type) {
			continue;
		}

		if state_key.as_str() == "*" {
			wildcard_types.insert(event_type);
			if let Ok(keys) = services
				.rooms
				.state_accessor
				.room_state_keys(room_id, event_type)
				.await
			{
				for key in keys {
					if let Ok(event) = services
						.rooms
						.state_accessor
						.room_state_get(room_id, event_type, &key)
						.await
					{
						required_state.push(Event::into_format(event));
					}
				}
			}
		} else if let Ok(event) = services
			.rooms
			.state_accessor
			.room_state_get(room_id, event_type, state_key)
			.await
		{
			required_state.push(Event::into_format(event));
		}
	}
	required_state
}

async fn collect_typing_events(
	services: &Services,
	sender_user: &UserId,
	body: &sync_events::v5::Request,
	todo_rooms: &TodoRooms,
	known_room_updates: &KnownRoomUpdates,
) -> Result<sync_events::v5::response::Typing> {
	if !body.extensions.typing.enabled.unwrap_or(false) {
		return Ok(sync_events::v5::response::Typing::default());
	}
	let typing = &body.extensions.typing;
	let scope = extension_scope(
		typing.lists.as_deref(),
		typing.rooms.as_deref(),
		body.lists.keys().map(String::as_str),
		known_room_updates,
	);
	if scope.is_empty() {
		return Ok(sync_events::v5::response::Typing::default());
	}

	let mut typing_response = sync_events::v5::response::Typing::default();
	for (room_id, (_, _, roomsince)) in todo_rooms {
		if !scope.contains(room_id) {
			continue;
		}

		if !services
			.rooms
			.state_cache
			.is_joined(sender_user, room_id)
			.await
		{
			continue;
		}

		if services.rooms.typing.last_typing_update(room_id).await? <= *roomsince {
			continue;
		}

		match services
			.rooms
			.typing
			.typing_users_for_user(room_id, sender_user)
			.await
		{
			| Ok(typing_users) => {
				typing_response.rooms.insert(
					room_id.to_owned(), // Already OwnedRoomId
					Raw::new(&SyncTypingEvent::new(TypingEventContent::new(typing_users)))?,
				);
			},
			| Err(e) => {
				warn!(%room_id, "Failed to get typing events for room: {}", e);
			},
		}
	}

	Ok(typing_response)
}

fn extension_scope<'a>(
	lists: Option<&'a [String]>,
	rooms: Option<&[ExtensionRoomConfig]>,
	all_list_ids: impl Iterator<Item = &'a str>,
	known_room_updates: &KnownRoomUpdates,
) -> BTreeSet<OwnedRoomId> {
	let list_ids: Vec<&str> = lists.map_or_else(
		|| all_list_ids.collect(),
		|lists| lists.iter().map(String::as_str).collect(),
	);
	let subscribed_rooms = known_room_updates
		.get("subscriptions")
		.cloned()
		.unwrap_or_default();
	let mut scope = BTreeSet::new();
	for list_id in list_ids {
		if let Some(rooms) = known_room_updates.get(list_id) {
			scope.extend(rooms.iter().cloned());
		}
	}
	if let Some(rooms) = rooms {
		for room in rooms {
			match room {
				| ExtensionRoomConfig::AllSubscribed =>
					scope.extend(subscribed_rooms.iter().cloned()),
				| ExtensionRoomConfig::Room(room_id) if subscribed_rooms.contains(room_id) => {
					scope.insert(room_id.clone());
				},
				| _ => {},
			}
		}
	} else {
		scope.extend(subscribed_rooms);
	}

	scope
}

async fn collect_account_data(
	services: &Services,
	(sender_user, _, globalsince, body): (&UserId, &DeviceId, u64, &sync_events::v5::Request),
) -> sync_events::v5::response::AccountData {
	let mut account_data = sync_events::v5::response::AccountData::default();

	if !body.extensions.account_data.enabled.unwrap_or(false) {
		return sync_events::v5::response::AccountData::default();
	}

	account_data.global = services
		.account_data
		.changes_since(None, sender_user, Some(globalsince), None)
		.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Global))
		.collect()
		.await;

	if let Some(rooms) = &body.extensions.account_data.rooms {
		for room in rooms {
			if let ExtensionRoomConfig::Room(room) = room {
				account_data.rooms.insert(
					room.clone(),
					services
						.account_data
						.changes_since(Some(room.as_ref()), sender_user, Some(globalsince), None)
						.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
						.collect()
						.await,
				);
			}
		}
	}

	account_data
}

async fn collect_e2ee<'a, Rooms>(
	services: &Services,
	(sender_user, sender_device, globalsince, body): (
		&UserId,
		&DeviceId,
		u64,
		&sync_events::v5::Request,
	),
	all_joined_rooms: Rooms,
) -> Result<sync_events::v5::response::E2EE>
where
	Rooms: Iterator<Item = &'a RoomId> + Send + 'a,
{
	if !body.extensions.e2ee.enabled.unwrap_or(false) {
		return Ok(sync_events::v5::response::E2EE::default());
	}
	let mut left_encrypted_users = HashSet::new(); // Users that have left any encrypted rooms the sender was in
	let mut device_list_changes = HashSet::new();
	let mut device_list_left = HashSet::new();
	// Look for device list updates of this account
	device_list_changes.extend(
		services
			.users
			.keys_changed(sender_user, Some(globalsince), None)
			.collect::<Vec<_>>()
			.await,
	);

	for room_id in all_joined_rooms {
		let Ok(current_shortstatehash) =
			services.rooms.state.get_room_shortstatehash(room_id).await
		else {
			error!("Room {room_id} has no state");
			continue;
		};

		let since_shortstatehash = async {
			pin! {
				let pdus_rev = services
					.rooms
					.timeline
					.pdus_rev(room_id, Some(PduCount::Normal(globalsince.saturating_sub(1))))
					.ignore_err();
			}

			let (count, pdu_at_last_sync_end) = pdus_rev.next().await?;

			if matches!(count, PduCount::Backfilled(_)) {
				None
			} else {
				Some(
					services
						.rooms
						.state_accessor
						.pdu_shortstatehash(&pdu_at_last_sync_end.event_id)
						.await
						.expect("pdu should have a shortstatehash"),
				)
			}
		}
		.await;

		let encrypted_room = services
			.rooms
			.state_accessor
			.state_get(current_shortstatehash, &StateEventType::RoomEncryption, "")
			.await
			.is_ok();

		if let Some(since_shortstatehash) = since_shortstatehash {
			// Skip if there are only timeline changes
			if since_shortstatehash == current_shortstatehash {
				continue;
			}

			let since_encryption = services
				.rooms
				.state_accessor
				.state_get(since_shortstatehash, &StateEventType::RoomEncryption, "")
				.await;

			let since_sender_member: Option<RoomMemberEventContent> = services
				.rooms
				.state_accessor
				.state_get_content(
					since_shortstatehash,
					&StateEventType::RoomMember,
					sender_user.as_str(),
				)
				.ok()
				.await;

			let joined_since_last_sync = since_sender_member
				.as_ref()
				.is_none_or(|member| member.membership != MembershipState::Join);

			let new_encrypted_room = encrypted_room && since_encryption.is_err();

			if encrypted_room {
				let current_state_ids: HashMap<_, OwnedEventId> = services
					.rooms
					.state_accessor
					.state_full_ids(current_shortstatehash)
					.collect()
					.await;

				let since_state_ids: HashMap<_, _> = services
					.rooms
					.state_accessor
					.state_full_ids(since_shortstatehash)
					.collect()
					.await;

				for (key, id) in current_state_ids {
					if since_state_ids.get(&key) != Some(&id) {
						let Ok(pdu) = services.rooms.timeline.get_pdu(&id).await else {
							error!("Pdu in state not found: {id}");
							continue;
						};
						if pdu.kind == TimelineEventType::RoomMember {
							if let Some(Ok(user_id)) = pdu.state_key.as_deref().map(UserId::parse)
							{
								if user_id == sender_user {
									continue;
								}

								let content: RoomMemberEventContent = pdu.get_content()?;
								match content.membership {
									| MembershipState::Join => {
										// A new user joined an encrypted room
										if !share_encrypted_room(
											services,
											sender_user,
											&user_id,
											Some(room_id),
										)
										.await
										{
											device_list_changes.insert(user_id.clone());
										}
									},
									| MembershipState::Leave => {
										// Write down users that have left encrypted rooms we
										// are in
										left_encrypted_users.insert(user_id.clone());
									},
									| _ => {},
								}
							}
						}
					}
				}
				if joined_since_last_sync || new_encrypted_room {
					// If the user is in a new encrypted room, give them all joined users
					device_list_changes.extend(
						services
						.rooms
						.state_cache
						.room_members(room_id)
						// Don't send key updates from the sender to the sender
						.ready_filter(|user_id| sender_user != *user_id)
						// Only send keys if the sender doesn't share an encrypted room with the target
						// already
						.filter_map(async |user_id| {
							share_encrypted_room(services, sender_user, &user_id, Some(room_id))
								.map(|res| res.or_some(user_id.clone()))
								.await
						})
						.collect::<Vec<_>>()
						.await,
					);
				}
			}
		}
		// Look for device list updates in this room
		device_list_changes.extend(
			services
				.users
				.room_keys_changed(room_id, Some(globalsince), None)
				.map(|(user_id, _)| user_id)
				.collect::<Vec<_>>()
				.await,
		);
	}

	for user_id in left_encrypted_users {
		let dont_share_encrypted_room =
			!share_encrypted_room(services, sender_user, &user_id, None).await;

		// If the user doesn't share an encrypted room with the target anymore, we need
		// to tell them
		if dont_share_encrypted_room {
			device_list_left.insert(user_id);
		}
	}

	Ok(assign!(sync_events::v5::response::E2EE::default(), {
		device_unused_fallback_key_types: None,

		device_one_time_keys_count: services
			.users
			.count_one_time_keys(sender_user, sender_device)
			.await,

		device_lists: assign!(DeviceLists::new(), {
			changed: device_list_changes.into_iter().collect(),
			left: device_list_left.into_iter().collect(),
		}),
	}))
}

async fn collect_to_device(
	services: &Services,
	(sender_user, sender_device, globalsince, body): SyncInfo<'_>,
	next_batch: u64,
) -> Option<sync_events::v5::response::ToDevice> {
	if !body.extensions.to_device.enabled.unwrap_or(false) {
		return None;
	}

	Some(assign!(sync_events::v5::response::ToDevice::default(), {
		next_batch: next_batch.to_string(),
		events: services
			.users
			.get_to_device_events(sender_user, sender_device, Some(globalsince), Some(next_batch))
			.map(at!(1))
			.collect()
			.await,
	}))
}

async fn collect_receipts(_services: &Services) -> sync_events::v5::response::Receipts {
	// TODO: get explicitly requested read receipts
	sync_events::v5::response::Receipts::default()
}

#[cfg(test)]
mod tests {
	use std::slice;

	use ruma::{owned_room_id, room_id};

	use super::*;

	fn custom_room_type() -> RoomTypeFilter { RoomTypeFilter::from(Some("com.example")) }

	fn response() -> sync_events::v5::Response { sync_events::v5::Response::new("1".to_owned()) }

	fn typing_scope_request() -> sync_events::v5::Request {
		let mut request = sync_events::v5::Request::new();
		request
			.lists
			.insert("first".to_owned(), sync_events::v5::request::List::default());
		request
			.lists
			.insert("second".to_owned(), sync_events::v5::request::List::default());
		request.room_subscriptions.insert(
			owned_room_id!("!subscription-a:example.com"),
			sync_events::v5::request::RoomSubscription::default(),
		);
		request.room_subscriptions.insert(
			owned_room_id!("!subscription-b:example.com"),
			sync_events::v5::request::RoomSubscription::default(),
		);
		request
	}

	fn known_typing_rooms() -> KnownRoomUpdates {
		BTreeMap::from([
			("first".to_owned(), BTreeSet::from([owned_room_id!("!list-a:example.com")])),
			("second".to_owned(), BTreeSet::from([owned_room_id!("!list-b:example.com")])),
			(
				"subscriptions".to_owned(),
				BTreeSet::from([
					owned_room_id!("!subscription-a:example.com"),
					owned_room_id!("!subscription-b:example.com"),
				]),
			),
		])
	}

	#[test]
	fn absent_bool_filter_matches_either_value() {
		assert!(matches_bool_filter(true, None));
		assert!(matches_bool_filter(false, None));
	}

	#[test]
	fn bool_filter_matches_only_its_own_value() {
		assert!(matches_bool_filter(true, Some(true)));
		assert!(!matches_bool_filter(false, Some(true)));
		assert!(matches_bool_filter(false, Some(false)));
		assert!(!matches_bool_filter(true, Some(false)));
	}

	#[test]
	fn absent_room_type_filters_match_every_type() {
		assert!(matches_room_type(&custom_room_type(), &[], &[]));
		assert!(matches_room_type(&RoomTypeFilter::Default, &[], &[]));
	}

	#[test]
	fn positive_room_type_matches_and_mismatches() {
		let custom = custom_room_type();
		assert!(matches_room_type(&custom, slice::from_ref(&custom), &[]));
		assert!(!matches_room_type(&custom, &[RoomTypeFilter::Default], &[]));
	}

	#[test]
	fn default_room_type_matches_untyped_rooms() {
		assert!(matches_room_type(&RoomTypeFilter::Default, &[RoomTypeFilter::Default], &[]));
	}

	#[test]
	fn negative_room_type_excludes_without_a_positive_filter() {
		assert!(!matches_room_type(&RoomTypeFilter::Space, &[], &[RoomTypeFilter::Space]));
		assert!(matches_room_type(&RoomTypeFilter::Default, &[], &[RoomTypeFilter::Space]));
	}

	#[test]
	fn negative_room_type_overrides_positive() {
		let custom = custom_room_type();
		assert!(!matches_room_type(&custom, slice::from_ref(&custom), slice::from_ref(&custom)));
	}

	#[test]
	fn fresh_response_is_empty() {
		assert!(response_is_empty(&response()));
	}

	#[test]
	fn account_data_is_not_empty() {
		let mut response = response();
		response
			.extensions
			.account_data
			.rooms
			.insert(owned_room_id!("!a:example.com"), vec![
				Raw::from_json_string("{}".to_owned()).unwrap(),
			]);

		assert!(!response_is_empty(&response));
	}

	#[test]
	fn a_room_with_no_events_is_still_empty() {
		let mut response = response();
		response
			.rooms
			.insert(owned_room_id!("!a:example.com"), sync_events::v5::response::Room::default());

		assert!(response_is_empty(&response));
	}

	#[test]
	fn a_receipt_makes_its_room_non_empty() {
		let room_id = room_id!("!a:example.com");
		let mut response = response();
		response
			.rooms
			.insert(room_id.to_owned(), sync_events::v5::response::Room::default());
		response
			.extensions
			.receipts
			.rooms
			.insert(room_id.to_owned(), Raw::from_json_string("{}".to_owned()).unwrap());

		assert!(!response_is_empty(&response));
	}

	#[test]
	fn typing_makes_a_response_non_empty() {
		let mut response = response();
		response.extensions.typing.rooms.insert(
			owned_room_id!("!a:example.com"),
			Raw::from_json_string("{}".to_owned()).unwrap(),
		);

		assert!(!response_is_empty(&response));
	}

	#[test]
	fn typing_scope_uses_selected_lists_and_subscriptions() {
		let request = typing_scope_request();

		assert_eq!(
			extension_scope(
				Some(&["first".to_owned()]),
				Some(&[
					ExtensionRoomConfig::Room(owned_room_id!("!list-b:example.com")),
					ExtensionRoomConfig::Room(owned_room_id!("!subscription-b:example.com")),
				]),
				request.lists.keys().map(String::as_str),
				&known_typing_rooms(),
			),
			BTreeSet::from([
				owned_room_id!("!list-a:example.com"),
				owned_room_id!("!subscription-b:example.com"),
			])
		);
	}

	#[test]
	fn typing_scope_defaults_to_all_lists_and_subscriptions() {
		let request = typing_scope_request();

		assert_eq!(
			extension_scope(
				None,
				None,
				request.lists.keys().map(String::as_str),
				&known_typing_rooms(),
			),
			BTreeSet::from([
				owned_room_id!("!list-a:example.com"),
				owned_room_id!("!list-b:example.com"),
				owned_room_id!("!subscription-a:example.com"),
				owned_room_id!("!subscription-b:example.com"),
			])
		);
	}

	#[test]
	fn to_device_without_events_is_empty() {
		let mut response = response();
		response.extensions.to_device = Some(sync_events::v5::response::ToDevice::default());

		assert!(response_is_empty(&response));
	}

	#[test]
	fn num_live_events_counts_the_new_normal_suffix() {
		let counts = [
			PduCount::Backfilled(-2),
			PduCount::Normal(8),
			PduCount::Normal(11),
			PduCount::Normal(12),
		];

		assert_eq!(num_live_events(counts.iter(), 0), 0);
		assert_eq!(num_live_events(counts.iter(), 10), 2);
		assert_eq!(num_live_events(counts.iter(), 12), 0);
	}

	#[test]
	fn num_live_events_stops_at_history() {
		let counts = [PduCount::Normal(11), PduCount::Normal(9), PduCount::Normal(12)];

		assert_eq!(num_live_events(counts.iter(), 10), 1);
	}
}
