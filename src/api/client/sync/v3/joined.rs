use std::collections::{BTreeMap, HashMap, HashSet};

use conduwuit::{
	Result, at, err, extract_variant,
	matrix::{
		Event,
		pdu::{PduCount, PduEvent},
	},
	utils::{
		BoolExt, IterStream, ReadyExt, TryFutureExtExt,
		math::ruma_from_u64,
		stream::{TryIgnore, WidebandExt},
	},
	warn,
};
use conduwuit_service::Services;
use futures::{
	FutureExt, StreamExt, TryFutureExt,
	future::{OptionFuture, join, join3, join4, try_join},
};
use ruma::{
	OwnedRoomId, OwnedUserId, RoomId, UserId,
	api::client::sync::sync_events::{
		UnreadNotificationsCount,
		v3::{Ephemeral, JoinedRoom, RoomAccountData, RoomSummary, State as RoomState, Timeline},
	},
	events::{
		AnyRawAccountDataEvent, AnySyncEphemeralRoomEvent, StateEventType,
		TimelineEventType::*,
		room::member::{MembershipState, RoomMemberEventContent},
	},
	serde::Raw,
	uint,
};
use service::rooms::short::ShortStateHash;

use super::{load_timeline, share_encrypted_room};
use crate::client::{
	ignored_filter,
	sync::v3::{
		DEFAULT_TIMELINE_LIMIT, DeviceListUpdates, SyncContext, prepare_lazily_loaded_members,
		state::{build_state_incremental, build_state_initial},
	},
};

/// Generate the sync response for a room the user is joined to.
#[tracing::instrument(
	name = "joined",
	level = "debug",
	skip_all,
	fields(
		room_id = ?room_id,
	),
)]
#[allow(clippy::too_many_arguments)]
pub(super) async fn load_joined_room(
	services: &Services,
	sync_context: SyncContext<'_>,
	ref room_id: OwnedRoomId,
) -> Result<(JoinedRoom, DeviceListUpdates)> {
	let SyncContext {
		syncing_user,
		last_sync_end_count,
		current_count,
		full_state,
		filter,
		..
	} = sync_context;
	let mut device_list_updates = DeviceListUpdates::new();

	// the room state as of `next_batch`.
	let current_shortstatehash = services
		.rooms
		.state
		.get_room_shortstatehash(room_id)
		.map_err(|_| err!(Database(error!("Room {room_id} has no state"))));

	// the room state as of the end of the last sync.
	// this will be None if we are doing an initial sync or if we just joined this
	// room.
	let last_sync_end_shortstatehash =
		OptionFuture::from(last_sync_end_count.map(|last_sync_end_count| {
			services
				.rooms
				.user
				.get_token_shortstatehash(room_id, last_sync_end_count)
				.ok()
		}))
		.map(Option::flatten)
		.map(Ok);

	let (current_shortstatehash, last_sync_end_shortstatehash) =
		try_join(current_shortstatehash, last_sync_end_shortstatehash).await?;

	// load recent timeline events.
	// if the filter specifies a limit, that will be used, otherwise
	// `DEFAULT_TIMELINE_LIMIT` will be used. `DEFAULT_TIMELINE_LIMIT` will also be
	// used if the limit is somehow greater than usize::MAX.

	let timeline_limit = filter
		.room
		.timeline
		.limit
		.and_then(|limit| limit.try_into().ok())
		.unwrap_or(DEFAULT_TIMELINE_LIMIT);

	let timeline = load_timeline(
		services,
		syncing_user,
		room_id,
		last_sync_end_count.map(PduCount::Normal),
		Some(PduCount::Normal(current_count)),
		timeline_limit,
	);

	let receipt_events = services
		.rooms
		.read_receipt
		.readreceipts_since(room_id, last_sync_end_count)
		.filter_map(|(read_user, _, edu)| async move {
			services
				.users
				.user_is_ignored(read_user, syncing_user)
				.await
				.or_some((read_user.to_owned(), edu))
		})
		.collect::<HashMap<OwnedUserId, Raw<AnySyncEphemeralRoomEvent>>>()
		.map(Ok);

	let (timeline, receipt_events) = try_join(timeline, receipt_events).boxed().await?;

	// the state at the beginning of the timeline
	let timeline_start_shortstatehash = async {
		if let Some((_, pdu)) = timeline.pdus.front() {
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

	let last_notification_read: OptionFuture<_> = timeline
		.pdus
		.is_empty()
		.then(|| {
			services
				.rooms
				.user
				.last_notification_read(syncing_user, room_id)
		})
		.into();

	// the syncing user's membership event during the last sync.
	// this will be None if `previous_sync_end_shortstatehash` is None.
	let membership_during_previous_sync: OptionFuture<_> = last_sync_end_shortstatehash
		.map(|shortstatehash| {
			services
				.rooms
				.state_accessor
				.state_get_content(
					shortstatehash,
					&StateEventType::RoomMember,
					syncing_user.as_str(),
				)
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
		membership_during_previous_sync,
		timeline_start_shortstatehash,
		is_encrypted_room,
	) = join4(
		last_notification_read,
		membership_during_previous_sync,
		timeline_start_shortstatehash,
		is_encrypted_room,
	)
	.await;

	// TODO: If the requesting user got state-reset out of the room, this
	// will be `true` when it shouldn't be. this function should never be called
	// in that situation, but it may be if the membership cache didn't get updated.
	// the root cause of this needs to be addressed
	let joined_since_last_sync = membership_during_previous_sync.flatten().is_none_or(
		|content: RoomMemberEventContent| content.membership != MembershipState::Join,
	);

	// the timeline should always include at least one PDU if the syncing user
	// joined since the last sync, that being the syncing user's join event. if
	// it's empty something is wrong.
	if joined_since_last_sync && timeline.pdus.is_empty() {
		warn!("timeline for newly joined room is empty");
	}

	// the user IDs of members whose membership needs to be sent to the client, if
	// lazy-loading is enabled.
	let lazily_loaded_members =
		prepare_lazily_loaded_members(services, sync_context, room_id, timeline.senders()).await;

	// compute the state delta between the previous sync and this sync.
	let state_events = match (last_sync_end_count, last_sync_end_shortstatehash) {
		/*
		if `last_sync_end_count` is Some (meaning this is an incremental sync), and `last_sync_end_shortstatehash`
		is Some (meaning the syncing user didn't just join this room for the first time ever), and `full_state` is false,
		then use `build_state_incremental`.
		*/
		| (Some(last_sync_end_count), Some(last_sync_end_shortstatehash)) if !full_state =>
			build_state_incremental(
				services,
				syncing_user,
				room_id,
				PduCount::Normal(last_sync_end_count),
				last_sync_end_shortstatehash,
				timeline_start_shortstatehash,
				current_shortstatehash,
				&timeline,
				lazily_loaded_members.as_ref(),
			)
			.boxed()
			.await?,
		/*
		otherwise use `build_state_initial`. note that this branch will be taken if the user joined this room since the last sync
		for the first time ever, because in that case we have no `last_sync_end_shortstatehash` and can't correctly calculate
		the state using the incremental sync algorithm.
		*/
		| _ =>
			build_state_initial(
				services,
				syncing_user,
				timeline_start_shortstatehash,
				lazily_loaded_members.as_ref(),
			)
			.boxed()
			.await?,
	};

	// for incremental syncs, calculate updates to E2EE device lists
	if last_sync_end_count.is_some() && is_encrypted_room {
		extend_device_list_updates(
			services,
			sync_context,
			room_id,
			&mut device_list_updates,
			&state_events,
			joined_since_last_sync,
		)
		.await;
	}

	/*
	build the `summary` field of the room object. this is necessary if:
	1. the syncing user joined this room since the last sync, because their client doesn't have a summary for this room yet, or
	2. we're going to sync a membership event in either `state` or `timeline`, because that event may impact
	   the joined/invited counts in the summary
	*/
	let sending_membership_events = timeline
		.pdus
		.iter()
		.map(|(_, pdu)| pdu)
		.chain(state_events.iter())
		.any(|event| event.kind == RoomMember);

	let summary = if sending_membership_events || joined_since_last_sync {
		build_room_summary(services, room_id, syncing_user, current_shortstatehash).await?
	} else {
		RoomSummary::default()
	};

	// the prev_batch token for the response
	let prev_batch = timeline.pdus.front().map(at!(0));

	let filtered_timeline = timeline
		.pdus
		.into_iter()
		.stream()
		// filter out ignored events from the timeline
		.wide_filter_map(|item| ignored_filter(services, item, syncing_user))
		.map(at!(1))
		.map(Event::into_format)
		.collect::<Vec<_>>();

	let account_data_events = services
		.account_data
		.changes_since(Some(room_id), syncing_user, last_sync_end_count, Some(current_count))
		.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
		.collect();

	/*
	send notification counts if:
	1. this is an initial sync, or
	2. the user hasn't seen any notifications, or
	3. the last notification the user saw has changed since the last sync
	*/
	let send_notification_counts = last_notification_read.is_none_or(|last_notification_read| {
		last_sync_end_count
			.is_none_or(|last_sync_end_count| last_notification_read > last_sync_end_count)
	});

	let notification_count: OptionFuture<_> = send_notification_counts
		.then(|| {
			services
				.rooms
				.user
				.notification_count(syncing_user, room_id)
				.map(TryInto::try_into)
				.unwrap_or(uint!(0))
		})
		.into();

	let highlight_count: OptionFuture<_> = send_notification_counts
		.then(|| {
			services
				.rooms
				.user
				.highlight_count(syncing_user, room_id)
				.map(TryInto::try_into)
				.unwrap_or(uint!(0))
		})
		.into();

	let typing_events = services
		.rooms
		.typing
		.last_typing_update(room_id)
		.and_then(|count| async move {
			if last_sync_end_count.is_some_and(|last_sync_end_count| count <= last_sync_end_count)
			{
				return Ok(Vec::<Raw<AnySyncEphemeralRoomEvent>>::new());
			}

			let typings = services
				.rooms
				.typing
				.typings_event_for_user(room_id, syncing_user)
				.await?;

			Ok(vec![serde_json::from_str(&serde_json::to_string(&typings)?)?])
		})
		.unwrap_or(Vec::new());

	let unread_notifications = join(notification_count, highlight_count);
	let events = join3(filtered_timeline, account_data_events, typing_events);
	let (unread_notifications, events) = join(unread_notifications, events).boxed().await;

	let (timeline_events, account_data_events, typing_events) = events;
	let (notification_count, highlight_count) = unread_notifications;

	let last_privateread_update = if let Some(last_sync_end_count) = last_sync_end_count {
		services
			.rooms
			.read_receipt
			.last_privateread_update(syncing_user, room_id)
			.await > last_sync_end_count
	} else {
		true
	};

	let private_read_event = if last_privateread_update {
		services
			.rooms
			.read_receipt
			.private_read_get(room_id, syncing_user)
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

	// save the room state at this sync to use during the next sync
	services
		.rooms
		.user
		.associate_token_shortstatehash(room_id, current_count, current_shortstatehash)
		.await;

	let joined_room = JoinedRoom {
		account_data: RoomAccountData { events: account_data_events },
		summary,
		unread_notifications: UnreadNotificationsCount { highlight_count, notification_count },
		timeline: Timeline {
			// mirror Synapse behavior by setting `limited` if the user joined since the last sync
			limited: timeline.limited || joined_since_last_sync,
			prev_batch: prev_batch.as_ref().map(ToString::to_string),
			events: timeline_events,
		},
		state: RoomState {
			events: state_events.into_iter().map(Event::into_format).collect(),
		},
		ephemeral: Ephemeral { events: edus },
		unread_thread_notifications: BTreeMap::new(),
	};

	Ok((joined_room, device_list_updates))
}

async fn extend_device_list_updates(
	services: &Services,
	SyncContext {
		syncing_user,
		last_sync_end_count: since,
		current_count,
		..
	}: SyncContext<'_>,
	room_id: &RoomId,
	device_list_updates: &mut DeviceListUpdates,
	state_events: &Vec<PduEvent>,
	joined_since_last_sync: bool,
) {
	// add users with changed keys to the `changed` list
	services
		.users
		.room_keys_changed(room_id, since, Some(current_count))
		.map(at!(0))
		.map(ToOwned::to_owned)
		.ready_for_each(|user_id| {
			device_list_updates.changed.insert(user_id);
		})
		.await;

	// add users who now share encrypted rooms to `changed` and
	// users who no longer share encrypted rooms to `left`
	for state_event in state_events {
		if state_event.kind == RoomMember {
			let Some(content): Option<RoomMemberEventContent> = state_event.get_content().ok()
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
					let shares_encrypted_room =
						share_encrypted_room(services, syncing_user, &user_id, Some(room_id))
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

/// Build the `summary` field of the room object, which includes
/// the number of joined and invited users and the room's heroes.
async fn build_room_summary(
	services: &Services,
	room_id: &RoomId,
	syncing_user: &UserId,
	current_shortstatehash: ShortStateHash,
) -> Result<RoomSummary> {
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

	let has_name = services
		.rooms
		.state_accessor
		.state_contains_type(current_shortstatehash, &StateEventType::RoomName);

	let has_canonical_alias = services
		.rooms
		.state_accessor
		.state_contains_type(current_shortstatehash, &StateEventType::RoomCanonicalAlias);

	let (joined_member_count, invited_member_count, has_name, has_canonical_alias) =
		join4(joined_member_count, invited_member_count, has_name, has_canonical_alias).await;

	// only send heroes if the room has neither a name nor a canonical alias
	let heroes: OptionFuture<_> = (!(has_name || has_canonical_alias))
		.then(|| build_heroes(services, room_id, syncing_user, current_shortstatehash))
		.into();

	Ok(RoomSummary {
		heroes: heroes
			.await
			.map(|heroes| heroes.into_iter().collect())
			.unwrap_or_default(),
		joined_member_count: Some(ruma_from_u64(joined_member_count)),
		invited_member_count: Some(ruma_from_u64(invited_member_count)),
	})
}

/// Fetch the user IDs to include in the `m.heroes` property of the room
/// summary.
async fn build_heroes(
	services: &Services,
	room_id: &RoomId,
	syncing_user: &UserId,
	current_shortstatehash: ShortStateHash,
) -> HashSet<OwnedUserId> {
	const MAX_HERO_COUNT: usize = 5;

	// fetch joined members from the state cache first
	let joined_members_stream = services
		.rooms
		.state_cache
		.room_members(room_id)
		.map(ToOwned::to_owned);

	// then fetch invited members
	let invited_members_stream = services
		.rooms
		.state_cache
		.room_members_invited(room_id)
		.map(ToOwned::to_owned);

	// then as a last resort fetch every membership event
	let all_members_stream = services
		.rooms
		.short
		.multi_get_statekey_from_short(
			services
				.rooms
				.state_accessor
				.state_full_shortids(current_shortstatehash)
				.ignore_err()
				.ready_filter_map(|(key, _)| Some(key)),
		)
		.ignore_err()
		.ready_filter_map(|(event_type, state_key)| {
			if event_type == StateEventType::RoomMember {
				state_key.to_string().try_into().ok()
			} else {
				None
			}
		});

	joined_members_stream
		.chain(invited_members_stream)
		.chain(all_members_stream)
		// the hero list should never include the syncing user
		.ready_filter(|user_id| user_id != syncing_user)
		.take(MAX_HERO_COUNT)
		.collect()
		.await
}
