use std::{
	cmp::{self},
	collections::{BTreeMap, HashMap, HashSet},
	time::Duration,
};

use axum::extract::State;
use conduwuit::{
	Result, at, err, error, extract_variant, is_equal_to,
	matrix::{
		Event,
		pdu::{EventHash, PduCount, PduEvent},
	},
	ref_at,
	result::FlatOk,
	utils::{
		self, BoolExt, FutureBoolExt, IterStream, ReadyExt, TryFutureExtExt,
		future::{OptionStream, ReadyEqExt},
		math::ruma_from_u64,
		stream::{BroadbandExt, Tools, WidebandExt},
	},
	warn,
};
use conduwuit_service::{
	Services,
	rooms::{
		lazy_loading,
		lazy_loading::{Options, Witness},
		short::ShortStateHash,
	},
};
use futures::{
	FutureExt, StreamExt, TryFutureExt,
	future::{OptionFuture, join, join3, join4, join5, try_join4},
	pin_mut,
};
use ruma::{
	DeviceId, EventId, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId,
	api::client::{
		filter::FilterDefinition,
		sync::sync_events::{
			self, DeviceLists, UnreadNotificationsCount,
			v3::{
				Ephemeral, Filter, GlobalAccountData, InviteState, InvitedRoom, JoinedRoom,
				KnockState, KnockedRoom, LeftRoom, Presence, RoomAccountData, RoomSummary, Rooms,
				State as RoomState, Timeline, ToDevice,
			},
		},
		uiaa::UiaaResponse,
	},
	events::{
		AnyRawAccountDataEvent, AnySyncEphemeralRoomEvent, StateEventType,
		TimelineEventType::*,
		presence::{PresenceEvent, PresenceEventContent},
		room::member::{MembershipState, RoomMemberEventContent},
	},
	serde::Raw,
	uint,
};
use service::rooms::short::{ShortEventId, ShortStateKey};
use tracing::trace;

use super::{load_timeline, share_encrypted_room};
use crate::{
	Ruma, RumaResponse,
	client::{TimelinePdus, ignored_filter, is_ignored_invite},
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
		.broad_filter_map(|room_id| {
			load_joined_room(services, context, room_id.clone())
				.map_ok(move |(joined_room, updates)| (room_id, joined_room, updates))
				.ok()
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
		.broad_filter_map(|(room_id, _)| {
			handle_left_room(services, context, room_id.clone())
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

#[tracing::instrument(
	name = "left",
	level = "debug",
	skip_all,
	fields(
		room_id = %room_id,
		full = %full_state,
	),
)]
#[allow(clippy::too_many_arguments)]
async fn handle_left_room(
	services: &Services,
	SyncContext {
		sender_user,
		since,
		next_batch,
		full_state,
		filter,
		..
	}: SyncContext<'_>,
	ref room_id: OwnedRoomId,
) -> Result<Option<LeftRoom>> {
	let left_count = services
		.rooms
		.state_cache
		.get_left_count(room_id, sender_user)
		.await
		.ok();

	// Left before last sync
	let include_leave = filter.room.include_leave;
	if (since >= left_count && !include_leave) || Some(next_batch) < left_count {
		return Ok(None);
	}

	let is_not_found = services.rooms.metadata.exists(room_id).eq(&false);

	let is_disabled = services.rooms.metadata.is_disabled(room_id);

	let is_banned = services.rooms.metadata.is_banned(room_id);

	pin_mut!(is_not_found, is_disabled, is_banned);
	if is_not_found.or(is_disabled).or(is_banned).await {
		// This is just a rejected invite, not a room we know
		// Insert a leave event anyways for the client
		let event = PduEvent {
			event_id: EventId::new(services.globals.server_name()),
			sender: sender_user.to_owned(),
			origin: None,
			origin_server_ts: utils::millis_since_unix_epoch()
				.try_into()
				.expect("Timestamp is valid js_int value"),
			kind: RoomMember,
			content: serde_json::from_str(r#"{"membership":"leave"}"#)
				.expect("this is valid JSON"),
			state_key: Some(sender_user.as_str().into()),
			unsigned: None,
			// The following keys are dropped on conversion
			room_id: Some(room_id.clone()),
			prev_events: vec![],
			depth: uint!(1),
			auth_events: vec![],
			redacts: None,
			hashes: EventHash { sha256: String::new() },
			signatures: None,
		};

		return Ok(Some(LeftRoom {
			account_data: RoomAccountData { events: Vec::new() },
			timeline: Timeline {
				limited: false,
				prev_batch: Some(next_batch.to_string()),
				events: Vec::new(),
			},
			state: RoomState { events: vec![event.into_format()] },
		}));
	}

	let mut left_state_events = Vec::new();

	let since_state_ids = async {
		let since_shortstatehash = services
			.rooms
			.user
			.get_token_shortstatehash(room_id, since?)
			.ok()
			.await?;

		services
			.rooms
			.state_accessor
			.state_full_ids(since_shortstatehash)
			.collect::<HashMap<_, OwnedEventId>>()
			.map(Some)
			.await
	}
	.await
	.unwrap_or_default();

	let Ok(left_event_id): Result<OwnedEventId> = services
		.rooms
		.state_accessor
		.room_state_get_id(room_id, &StateEventType::RoomMember, sender_user.as_str())
		.await
	else {
		warn!("Left {room_id} but no left state event");
		return Ok(None);
	};

	let Ok(left_shortstatehash) = services
		.rooms
		.state_accessor
		.pdu_shortstatehash(&left_event_id)
		.await
	else {
		warn!(event_id = %left_event_id, "Leave event has no state in {room_id}");
		return Ok(None);
	};

	let mut left_state_ids: HashMap<_, _> = services
		.rooms
		.state_accessor
		.state_full_ids(left_shortstatehash)
		.collect()
		.await;

	let leave_shortstatekey = services
		.rooms
		.short
		.get_or_create_shortstatekey(&StateEventType::RoomMember, sender_user.as_str())
		.await;

	left_state_ids.insert(leave_shortstatekey, left_event_id);

	for (shortstatekey, event_id) in left_state_ids {
		if full_state || since_state_ids.get(&shortstatekey) != Some(&event_id) {
			let (event_type, state_key) = services
				.rooms
				.short
				.get_statekey_from_short(shortstatekey)
				.await?;

			if filter.room.state.lazy_load_options.is_enabled()
				&& event_type == StateEventType::RoomMember
				&& !full_state
				&& state_key
					.as_str()
					.try_into()
					.is_ok_and(|user_id: &UserId| sender_user != user_id)
			{
				continue;
			}

			let Ok(pdu) = services.rooms.timeline.get_pdu(&event_id).await else {
				error!("Pdu in state not found: {event_id}");
				continue;
			};

			if !include_leave && pdu.sender == sender_user {
				continue;
			}

			left_state_events.push(pdu.into_format());
		}
	}

	Ok(Some(LeftRoom {
		account_data: RoomAccountData { events: Vec::new() },
		timeline: Timeline {
			// TODO: support left timeline events so we dont need to set limited to true
			limited: true,
			prev_batch: Some(next_batch.to_string()),
			events: Vec::new(), // and so we dont need to set this to empty vec
		},
		state: RoomState { events: left_state_events },
	}))
}

#[tracing::instrument(
	name = "joined",
	level = "debug",
	skip_all,
	fields(
		room_id = ?room_id,
	),
)]
#[allow(clippy::too_many_arguments)]
async fn load_joined_room(
	services: &Services,
	SyncContext {
		sender_user,
		sender_device,
		since,
		next_batch,
		full_state,
		filter,
	}: SyncContext<'_>,
	ref room_id: OwnedRoomId,
) -> Result<(JoinedRoom, DeviceListUpdates)> {
	let mut device_list_updates = DeviceListUpdates::new();
	let sincecount = since.map(PduCount::Normal);
	let next_batchcount = PduCount::Normal(next_batch);

	// the shortstatehash of the room's state right now
	let current_shortstatehash = services
		.rooms
		.state
		.get_room_shortstatehash(room_id)
		.map_err(|_| err!(Database(error!("Room {room_id} has no state"))));

	// the shortstatehash of what the room's state was when the `since` token was
	// issued
	let since_shortstatehash = OptionFuture::from(since.map(|since| {
		services
			.rooms
			.user
			.get_token_shortstatehash(room_id, since)
			.ok()
	}))
	.map(|v| Ok(v.flatten()));

	let timeline = load_timeline(
		services,
		sender_user,
		room_id,
		sincecount,
		Some(next_batchcount),
		10_usize,
	);

	let receipt_events = services
		.rooms
		.read_receipt
		.readreceipts_since(room_id, since)
		.filter_map(|(read_user, _, edu)| async move {
			services
				.users
				.user_is_ignored(read_user, sender_user)
				.await
				.or_some((read_user.to_owned(), edu))
		})
		.collect::<HashMap<OwnedUserId, Raw<AnySyncEphemeralRoomEvent>>>()
		.map(Ok);

	let (current_shortstatehash, since_shortstatehash, timeline, receipt_events) =
		try_join4(current_shortstatehash, since_shortstatehash, timeline, receipt_events)
			.boxed()
			.await?;

	let TimelinePdus { pdus: timeline_pdus, limited } = timeline;
	let is_initial_sync = since_shortstatehash.is_none();

	let timeline_start_shortstatehash = async {
		if let Some((_, pdu)) = timeline_pdus.first() {
			if let Ok(shortstatehash) = services
				.rooms
				.state_accessor
				.pdu_shortstatehash(&pdu.event_id)
				.await
			{
				return shortstatehash;
			}
		}

		current_shortstatehash
	};

	let last_notification_read: OptionFuture<_> = timeline_pdus
		.is_empty()
		.then(|| {
			services
				.rooms
				.user
				.last_notification_read(sender_user, room_id)
		})
		.into();

	let since_sender_member: OptionFuture<_> = since_shortstatehash
		.map(|short| {
			services
				.rooms
				.state_accessor
				.state_get_content(short, &StateEventType::RoomMember, sender_user.as_str())
				.ok()
		})
		.into();

	let is_encrypted_room = services
		.rooms
		.state_accessor
		.state_get(current_shortstatehash, &StateEventType::RoomEncryption, "")
		.is_ok();

	let (
		last_notification_read,
		since_sender_member,
		timeline_start_shortstatehash,
		is_encrypted_room,
	) = join4(
		last_notification_read,
		since_sender_member,
		timeline_start_shortstatehash,
		is_encrypted_room,
	)
	.await;

	let joined_since_last_sync =
		since_sender_member
			.flatten()
			.is_none_or(|content: RoomMemberEventContent| {
				content.membership != MembershipState::Join
			});

	let lazy_loading_enabled = (filter.room.state.lazy_load_options.is_enabled()
		|| filter.room.timeline.lazy_load_options.is_enabled())
		&& !full_state;

	let lazy_loading_context = &lazy_loading::Context {
		user_id: sender_user,
		device_id: Some(sender_device),
		room_id,
		token: since,
		options: Some(&filter.room.state.lazy_load_options),
	};

	let lazy_loading_witness = OptionFuture::from(lazy_loading_enabled.then(|| {
		let witness: Witness = timeline_pdus
			.iter()
			.map(ref_at!(1))
			.map(Event::sender)
			.map(Into::into)
			.chain(receipt_events.keys().map(Into::into))
			.collect();

		services
			.rooms
			.lazy_loading
			.witness_retain(witness, lazy_loading_context)
	}))
	.await;

	/* replace with multiple steps
		glossary for my own sanity:
		- full state: every state event from the start of the room to the start of the timeline
		- incremental state: state events from `since` to the start of the timeline
		- state_events: the `state` key on the JSON object we return

		if initial sync or full_state:
			get full state
			use full state as state_events
		else if TL is limited:
			get incremental state
			use incremental state as state_events
			if encryption is enabled:
				use incremental state to extend device list
		else:
			state_events is empty
		compute counts and heroes from state_events
	*/
	let mut state_events = if is_initial_sync || full_state {
		// reset lazy loading state on initial sync
		if is_initial_sync {
			services
				.rooms
				.lazy_loading
				.reset(lazy_loading_context)
				.await;
		}

		calculate_state_initial(
			services,
			sender_user,
			timeline_start_shortstatehash,
			lazy_loading_witness.as_ref(),
		)
		.boxed()
		.await?
	} else if limited {
		let state_incremental = calculate_state_incremental(
			services,
			sender_user,
			since_shortstatehash,
			timeline_start_shortstatehash,
			lazy_loading_witness.as_ref(),
		)
		.boxed()
		.await?;

		// calculate device list updates for E2EE
		if is_encrypted_room {
			// add users with changed keys to the `changed` list
			services
				.users
				.room_keys_changed(room_id, since, Some(next_batch))
				.map(at!(0))
				.map(ToOwned::to_owned)
				.ready_for_each(|user_id| {
					device_list_updates.changed.insert(user_id);
				})
				.await;

			// add users who now share encrypted rooms to `changed` and
			// users who no longer share encrypted rooms to `left`
			for state_event in &state_incremental {
				if state_event.kind == RoomMember {
					let Some(content): Option<RoomMemberEventContent> =
						state_event.get_content().ok()
					else {
						continue;
					};

					let Some(user_id): Option<OwnedUserId> = state_event
						.state_key
						.as_ref()
						.and_then(|key| key.parse().ok())
					else {
						continue;
					};

					{
						use MembershipState::*;

						if matches!(content.membership, Leave | Join) {
							let shares_encrypted_room = share_encrypted_room(
								services,
								sender_user,
								&user_id,
								Some(room_id),
							)
							.await;
							match content.membership {
								| Leave if !shares_encrypted_room => {
									device_list_updates.left.insert(user_id);
								},
								| Join if joined_since_last_sync || shares_encrypted_room => {
									device_list_updates.changed.insert(user_id);
								},
								| _ => (),
							}
						}
					}
				}
			}
		}

		state_incremental
	} else {
		vec![]
	};

	// only compute room counts and heroes (aka the summary) if the room's members
	// changed since the last sync
	let (joined_member_count, invited_member_count, heroes) =
		if state_events.iter().any(|event| event.kind == RoomMember) {
			calculate_counts(services, room_id, sender_user).await?
		} else {
			(None, None, None)
		};

	let is_sender_membership = |pdu: &PduEvent| {
		pdu.kind == StateEventType::RoomMember.into()
			&& pdu
				.state_key
				.as_deref()
				.is_some_and(is_equal_to!(sender_user.as_str()))
	};

	let joined_sender_member: Option<_> = (joined_since_last_sync && timeline_pdus.is_empty())
		.then(|| {
			state_events
				.iter()
				.position(is_sender_membership)
				.map(|pos| state_events.swap_remove(pos))
		})
		.flatten();

	let prev_batch = timeline_pdus
		.first()
		.map(at!(0))
		.or_else(|| joined_sender_member.is_some().and(since).map(Into::into));

	let timeline_pdus = timeline_pdus
		.into_iter()
		.stream()
		.wide_filter_map(|item| ignored_filter(services, item, sender_user))
		.map(at!(1))
		.chain(joined_sender_member.into_iter().stream())
		.map(Event::into_format)
		.collect::<Vec<_>>();

	let account_data_events = services
		.account_data
		.changes_since(Some(room_id), sender_user, since, Some(next_batch))
		.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
		.collect();

	let send_notification_counts =
		last_notification_read.is_none_or(|count| since.is_none_or(|since| count > since));

	let notification_count: OptionFuture<_> = send_notification_counts
		.then(|| {
			services
				.rooms
				.user
				.notification_count(sender_user, room_id)
				.map(TryInto::try_into)
				.unwrap_or(uint!(0))
		})
		.into();

	let highlight_count: OptionFuture<_> = send_notification_counts
		.then(|| {
			services
				.rooms
				.user
				.highlight_count(sender_user, room_id)
				.map(TryInto::try_into)
				.unwrap_or(uint!(0))
		})
		.into();

	let typing_events = services
		.rooms
		.typing
		.last_typing_update(room_id)
		.and_then(|count| async move {
			if since.is_some_and(|since| count <= since) {
				return Ok(Vec::<Raw<AnySyncEphemeralRoomEvent>>::new());
			}

			let typings = services
				.rooms
				.typing
				.typings_all(room_id, sender_user)
				.await?;

			Ok(vec![serde_json::from_str(&serde_json::to_string(&typings)?)?])
		})
		.unwrap_or(Vec::new());

	let unread_notifications = join(notification_count, highlight_count);
	let events = join3(timeline_pdus, account_data_events, typing_events);
	let (unread_notifications, events) = join(unread_notifications, events).boxed().await;

	let (room_events, account_data_events, typing_events) = events;
	let (notification_count, highlight_count) = unread_notifications;

	let last_privateread_update = if let Some(since) = since {
		services
			.rooms
			.read_receipt
			.last_privateread_update(sender_user, room_id)
			.await > since
	} else {
		true
	};

	let private_read_event = if last_privateread_update {
		services
			.rooms
			.read_receipt
			.private_read_get(room_id, sender_user)
			.await
			.ok()
	} else {
		None
	};

	let edus: Vec<Raw<AnySyncEphemeralRoomEvent>> = receipt_events
		.into_values()
		.chain(typing_events.into_iter())
		.chain(private_read_event.into_iter())
		.collect();

	// Save the state after this sync so we can send the correct state diff next
	// sync
	services
		.rooms
		.user
		.associate_token_shortstatehash(room_id, next_batch, current_shortstatehash)
		.await;

	let joined_room = JoinedRoom {
		account_data: RoomAccountData { events: account_data_events },
		summary: RoomSummary {
			joined_member_count: joined_member_count.map(ruma_from_u64),
			invited_member_count: invited_member_count.map(ruma_from_u64),
			heroes: heroes
				.into_iter()
				.flatten()
				.map(TryInto::try_into)
				.filter_map(Result::ok)
				.collect(),
		},
		unread_notifications: UnreadNotificationsCount { highlight_count, notification_count },
		timeline: Timeline {
			limited,
			prev_batch: prev_batch.as_ref().map(ToString::to_string),
			events: room_events,
		},
		state: RoomState {
			events: state_events.into_iter().map(Event::into_format).collect(),
		},
		ephemeral: Ephemeral { events: edus },
		unread_thread_notifications: BTreeMap::new(),
	};

	Ok((joined_room, device_list_updates))
}

/// Calculate the "initial state", or all events from the start of the room up
/// to (but not including) the `current_shortstatehash`. If
/// `lazy_loading_witness` is `None`, lazy loading will be disabled.
#[tracing::instrument(
	name = "initial",
	level = "trace",
	skip_all,
	fields(current_shortstatehash)
)]
#[allow(clippy::too_many_arguments)]
async fn calculate_state_initial(
	services: &Services,
	sender_user: &UserId,
	current_shortstatehash: ShortStateHash,
	lazy_loading_witness: Option<&Witness>,
) -> Result<Vec<PduEvent>> {
	let (shortstatekeys, event_ids): (Vec<_>, Vec<_>) = services
		.rooms
		.state_accessor
		.state_full_ids(current_shortstatehash)
		.unzip()
		.await;

	trace!("event ids for initial sync @ {:?}: {:?}", current_shortstatehash, event_ids);

	services
		.rooms
		.short
		.multi_get_statekey_from_short(shortstatekeys.into_iter().stream())
		.zip(event_ids.into_iter().stream())
		.ready_filter_map(|item| Some((item.0.ok()?, item.1)))
		.ready_filter_map(|((event_type, state_key), event_id)| {
			let lazy = lazy_loading_witness.is_some_and(|witness| {
				event_type == StateEventType::RoomMember
					&& state_key.as_str().try_into().is_ok_and(|user_id: &UserId| {
						sender_user != user_id && !witness.contains(user_id)
					})
			});

			lazy.or_some(event_id)
		})
		.broad_filter_map(|event_id: OwnedEventId| async move {
			services.rooms.timeline.get_pdu(&event_id).await.ok()
		})
		.collect()
		.map(Ok)
		.await
}

/// Calculate the "incremental state", or all events from the
/// `since_shortstatehash` up to (but not including)
/// the `current_shortstatehash`. If `lazy_loading_witness` is `None`, lazy
/// loading will be disabled.
#[tracing::instrument(name = "incremental", level = "trace", skip_all)]
#[allow(clippy::too_many_arguments)]
async fn calculate_state_incremental<'a>(
	services: &Services,
	sender_user: &'a UserId,
	since_shortstatehash: Option<ShortStateHash>,
	current_shortstatehash: ShortStateHash,
	lazy_loading_witness: Option<&'a Witness>,
) -> Result<Vec<PduEvent>> {
	let since_shortstatehash = since_shortstatehash.unwrap_or(current_shortstatehash);

	let state_get_shorteventid = |user_id: &'a UserId| {
		services
			.rooms
			.state_accessor
			.state_get_shortid(
				current_shortstatehash,
				&StateEventType::RoomMember,
				user_id.as_str(),
			)
			.ok()
	};

	let lazy_state_ids: OptionFuture<_> = lazy_loading_witness
		.map(|witness| {
			StreamExt::into_future(
				witness
					.iter()
					.stream()
					.broad_filter_map(|user_id| state_get_shorteventid(user_id)),
			)
		})
		.into();

	let state_diff_shortids = services
		.rooms
		.state_accessor
		.state_added((since_shortstatehash, current_shortstatehash))
		.boxed();

	state_diff_shortids
		.broad_filter_map(|(shortstatekey, shorteventid)| async move {
			if lazy_loading_witness.is_none() {
				return Some(shorteventid);
			}

			lazy_filter(services, sender_user, shortstatekey, shorteventid).await
		})
		.chain(lazy_state_ids.stream())
		.broad_filter_map(|shorteventid| {
			services
				.rooms
				.short
				.get_eventid_from_short(shorteventid)
				.ok()
		})
		.broad_filter_map(|event_id: OwnedEventId| async move {
			services.rooms.timeline.get_pdu(&event_id).await.ok()
		})
		.collect::<Vec<_>>()
		.map(Ok)
		.await
}

async fn lazy_filter(
	services: &Services,
	sender_user: &UserId,
	shortstatekey: ShortStateKey,
	shorteventid: ShortEventId,
) -> Option<ShortEventId> {
	let (event_type, state_key) = services
		.rooms
		.short
		.get_statekey_from_short(shortstatekey)
		.await
		.ok()?;

	(event_type != StateEventType::RoomMember || state_key == sender_user.as_str())
		.then_some(shorteventid)
}

async fn calculate_counts(
	services: &Services,
	room_id: &RoomId,
	sender_user: &UserId,
) -> Result<(Option<u64>, Option<u64>, Option<Vec<OwnedUserId>>)> {
	let joined_member_count = services
		.rooms
		.state_cache
		.room_joined_count(room_id)
		.unwrap_or(0);

	let invited_member_count = services
		.rooms
		.state_cache
		.room_invited_count(room_id)
		.unwrap_or(0);

	let (joined_member_count, invited_member_count) =
		join(joined_member_count, invited_member_count).await;

	let small_room = joined_member_count.saturating_add(invited_member_count) <= 5;

	let heroes: OptionFuture<_> = small_room
		.then(|| calculate_heroes(services, room_id, sender_user))
		.into();

	Ok((Some(joined_member_count), Some(invited_member_count), heroes.await))
}

async fn calculate_heroes(
	services: &Services,
	room_id: &RoomId,
	sender_user: &UserId,
) -> Vec<OwnedUserId> {
	services
		.rooms
		.timeline
		.all_pdus(sender_user, room_id)
		.ready_filter(|(_, pdu)| pdu.kind == RoomMember)
		.fold_default(|heroes: Vec<_>, (_, pdu)| {
			fold_hero(heroes, services, room_id, sender_user, pdu)
		})
		.await
}

async fn fold_hero(
	mut heroes: Vec<OwnedUserId>,
	services: &Services,
	room_id: &RoomId,
	sender_user: &UserId,
	pdu: PduEvent,
) -> Vec<OwnedUserId> {
	let Some(user_id): Option<&UserId> =
		pdu.state_key.as_deref().map(TryInto::try_into).flat_ok()
	else {
		return heroes;
	};

	if user_id == sender_user {
		return heroes;
	}

	let Ok(content): Result<RoomMemberEventContent, _> = pdu.get_content() else {
		return heroes;
	};

	// The membership was and still is invite or join
	if !matches!(content.membership, MembershipState::Join | MembershipState::Invite) {
		return heroes;
	}

	if heroes.iter().any(is_equal_to!(user_id)) {
		return heroes;
	}

	let (is_invited, is_joined) = join(
		services.rooms.state_cache.is_invited(user_id, room_id),
		services.rooms.state_cache.is_joined(user_id, room_id),
	)
	.await;

	if !is_joined && is_invited {
		return heroes;
	}

	heroes.push(user_id.to_owned());
	heroes
}
