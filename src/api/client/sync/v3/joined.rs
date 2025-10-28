use std::collections::{BTreeMap, HashMap};

use conduwuit::{
	Result, at, err, extract_variant, is_equal_to,
	matrix::{
		Event,
		pdu::{PduCount, PduEvent},
	},
	result::FlatOk,
	utils::{
		BoolExt, IterStream, ReadyExt, TryFutureExtExt,
		math::ruma_from_u64,
		stream::{Tools, WidebandExt},
	},
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

use super::{load_timeline, share_encrypted_room};
use crate::client::{
	ignored_filter,
	sync::v3::{
		DeviceListUpdates, SyncContext, prepare_lazily_loaded_members,
		state::{calculate_state_incremental, calculate_state_initial},
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
		sender_user,
		since,
		next_batch,
		full_state,
		..
	} = sync_context;

	// the global count as of the end of the last sync.
	// this will be None if we are doing an initial sync.
	let previous_sync_end_count = since.map(PduCount::Normal);
	let next_batchcount = PduCount::Normal(next_batch);
	let mut device_list_updates = DeviceListUpdates::new();

	// the room state right now
	let current_shortstatehash = services
		.rooms
		.state
		.get_room_shortstatehash(room_id)
		.map_err(|_| err!(Database(error!("Room {room_id} has no state"))));

	// the room state as of the end of the last sync.
	// this will be None if we are doing an initial sync or if we just joined this
	// room.
	let previous_sync_end_shortstatehash = OptionFuture::from(since.map(|since| {
		services
			.rooms
			.user
			.get_token_shortstatehash(room_id, since)
			.ok()
	}))
	.map(Option::flatten)
	.map(Ok);

	let (current_shortstatehash, previous_sync_end_shortstatehash) =
		try_join(current_shortstatehash, previous_sync_end_shortstatehash).await?;

	let timeline = load_timeline(
		services,
		sender_user,
		room_id,
		previous_sync_end_count,
		Some(next_batchcount),
		services.config.incremental_sync_max_timeline_size,
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
				.last_notification_read(sender_user, room_id)
		})
		.into();

	// the syncing user's membership event during the last sync.
	// this will be None if `previous_sync_end_shortstatehash` is None.
	let membership_during_previous_sync: OptionFuture<_> = previous_sync_end_shortstatehash
		.map(|shortstatehash| {
			services
				.rooms
				.state_accessor
				.state_get_content(
					shortstatehash,
					&StateEventType::RoomMember,
					sender_user.as_str(),
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

	// the user IDs of members whose membership needs to be sent to the client, if
	// lazy-loading is enabled.
	let lazily_loaded_members =
		prepare_lazily_loaded_members(services, sync_context, room_id, timeline.senders()).await;

	/*
	compute the state delta between the previous sync and this sync. if this is an initial sync
	*or* we just joined this room, `calculate_state_initial` will be used, otherwise `calculate_state_incremental`
	will be used.
	*/
	let mut state_events = if let Some(previous_sync_end_count) = previous_sync_end_count
		&& let Some(previous_sync_end_shortstatehash) = previous_sync_end_shortstatehash
		&& !full_state
	{
		calculate_state_incremental(
			services,
			sender_user,
			room_id,
			previous_sync_end_count,
			previous_sync_end_shortstatehash,
			timeline_start_shortstatehash,
			current_shortstatehash,
			&timeline,
			lazily_loaded_members.as_ref(),
		)
		.boxed()
		.await?
	} else {
		calculate_state_initial(
			services,
			sender_user,
			timeline_start_shortstatehash,
			lazily_loaded_members.as_ref(),
		)
		.boxed()
		.await?
	};

	// for incremental syncs, calculate updates to E2EE device lists
	if previous_sync_end_count.is_some() && is_encrypted_room {
		calculate_device_list_updates(
			services,
			sync_context,
			room_id,
			&mut device_list_updates,
			&state_events,
			joined_since_last_sync,
		)
		.await;
	}

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

	// the membership event of the syncing user, if they joined since the last sync
	let sender_join_membership_event: Option<_> = (joined_since_last_sync
		&& timeline.pdus.is_empty())
	.then(|| {
		state_events
			.iter()
			.position(is_sender_membership)
			.map(|pos| state_events.swap_remove(pos))
	})
	.flatten();

	// the prev_batch token for the response
	let prev_batch = timeline.pdus.front().map(at!(0)).or_else(|| {
		sender_join_membership_event
			.is_some()
			.and(since)
			.map(Into::into)
	});

	let timeline_pdus = timeline
		.pdus
		.into_iter()
		.stream()
		// filter out ignored events from the timeline
		.wide_filter_map(|item| ignored_filter(services, item, sender_user))
		.map(at!(1))
		// if the syncing user just joined, add their membership event to the timeline
		.chain(sender_join_membership_event.into_iter().stream())
		.map(Event::into_format)
		.collect::<Vec<_>>();

	let account_data_events = services
		.account_data
		.changes_since(Some(room_id), sender_user, since, Some(next_batch))
		.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
		.collect();

	/*
	send notification counts if:
	1. this is an initial sync
	2. the user hasn't seen any notifications
	3. the last notification the user saw has changed since the last sync
	*/
	let send_notification_counts = last_notification_read.is_none_or(|last_notification_read| {
		since.is_none_or(|since| last_notification_read > since)
	});

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
				.typings_event_for_user(room_id, sender_user)
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

	// save the room state at this sync to use during the next sync
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
			// mirror Synapse behavior by setting `limited` if the user joined since the last sync
			limited: timeline.limited || joined_since_last_sync,
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

async fn calculate_device_list_updates(
	services: &Services,
	SyncContext { sender_user, since, next_batch, .. }: SyncContext<'_>,
	room_id: &RoomId,
	device_list_updates: &mut DeviceListUpdates,
	state_events: &Vec<PduEvent>,
	joined_since_last_sync: bool,
) {
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
						share_encrypted_room(services, sender_user, &user_id, Some(room_id))
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
