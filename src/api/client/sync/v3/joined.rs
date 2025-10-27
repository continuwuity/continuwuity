use std::{
	collections::{BTreeMap, BTreeSet, HashMap},
	ops::ControlFlow,
};

use conduwuit::{
	Result, at, err, extract_variant, is_equal_to,
	matrix::{
		Event,
		pdu::{PduCount, PduEvent},
	},
	ref_at,
	result::FlatOk,
	utils::{
		BoolExt, IterStream, ReadyExt, TryFutureExtExt,
		math::ruma_from_u64,
		stream::{BroadbandExt, Tools, TryIgnore, WidebandExt},
	},
};
use conduwuit_service::{
	Services,
	rooms::{
		lazy_loading,
		lazy_loading::{MemberSet, Options},
		short::ShortStateHash,
	},
};
use futures::{
	FutureExt, StreamExt, TryFutureExt,
	future::{OptionFuture, join, join3, join4, try_join},
};
use itertools::Itertools;
use ruma::{
	OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId,
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
use service::rooms::short::ShortEventId;
use tracing::trace;

use super::{load_timeline, share_encrypted_room};
use crate::client::{
	TimelinePdus, ignored_filter,
	sync::v3::{DeviceListUpdates, SyncContext},
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
	/*
	this is a large function with a lot of logic. we try to parallelize as much as possible
	by fetching data concurrently, so the code is roughly split into stages separated by calls to `join<n>`.

	1.  `current_shortstatehash` and `since_shortstatehash` are fetched from the DB. a shortstatehash is
		a token which identifies the state of the room at a point in time.
	2.  `load_timeline` is called to fetch timeline events that happened since `since`.
	3.
	*/

	let SyncContext {
		sender_user,
		sender_device,
		since,
		next_batch,
		full_state,
		filter,
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

	// lazy loading is only enabled if the filter allows for it and we aren't
	// requesting the full state
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

	// the user IDs of members whose membership needs to be sent to the client, if
	// lazy-loading is enabled.
	let lazily_loaded_members = OptionFuture::from(lazy_loading_enabled.then(|| {
		let witness: MemberSet = timeline
			.pdus
			.iter()
			.map(ref_at!(1))
			.map(Event::sender)
			.map(Into::into)
			.chain(receipt_events.keys().map(Into::into))
			.collect();

		services
			.rooms
			.lazy_loading
			.retain_lazy_members(witness, lazy_loading_context)
	}))
	.await;

	// reset lazy loading state on initial sync
	if previous_sync_end_count.is_none() {
		services
			.rooms
			.lazy_loading
			.reset(lazy_loading_context)
			.await;
	}

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
			limited: timeline.limited,
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

/// Calculate the state events to include in an initial sync response.
///
/// If lazy-loading is enabled (`lazily_loaded_members` is Some), the returned
/// Vec will include the membership events of exclusively the members in
/// `lazily_loaded_members`.
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
	timeline_start_shortstatehash: ShortStateHash,
	lazily_loaded_members: Option<&MemberSet>,
) -> Result<Vec<PduEvent>> {
	// load the keys and event IDs of the state events at the start of the timeline
	let (shortstatekeys, event_ids): (Vec<_>, Vec<_>) = services
		.rooms
		.state_accessor
		.state_full_ids(timeline_start_shortstatehash)
		.unzip()
		.await;

	trace!("performing initial sync of {} state events", event_ids.len());

	services
		.rooms
		.short
		// look up the full state keys
		.multi_get_statekey_from_short(shortstatekeys.into_iter().stream())
		.zip(event_ids.into_iter().stream())
		.ready_filter_map(|item| Some((item.0.ok()?, item.1)))
		.ready_filter_map(|((event_type, state_key), event_id)| {
			if let Some(lazily_loaded_members) = lazily_loaded_members {
				/*
				if lazy loading is enabled, filter out membership events which aren't for a user
				included in `lazily_loaded_members` or for the user requesting the sync.
				*/
				let event_is_redundant = event_type == StateEventType::RoomMember
					&& state_key.as_str().try_into().is_ok_and(|user_id: &UserId| {
						sender_user != user_id && !lazily_loaded_members.contains(user_id)
					});

				event_is_redundant.or_some(event_id)
			} else {
				Some(event_id)
			}
		})
		.broad_filter_map(|event_id: OwnedEventId| async move {
			services.rooms.timeline.get_pdu(&event_id).await.ok()
		})
		.collect()
		.map(Ok)
		.await
}

/// Calculate the state events to include in an incremental sync response.
///
/// If lazy-loading is enabled (`lazily_loaded_members` is Some), the returned
/// Vec will include the membership events of all the members in
/// `lazily_loaded_members`.
#[tracing::instrument(name = "incremental", level = "trace", skip_all)]
#[allow(clippy::too_many_arguments)]
async fn calculate_state_incremental<'a>(
	services: &Services,
	sender_user: &'a UserId,
	room_id: &RoomId,
	previous_sync_end_count: PduCount,
	previous_sync_end_shortstatehash: ShortStateHash,
	timeline_start_shortstatehash: ShortStateHash,
	timeline_end_shortstatehash: ShortStateHash,
	timeline: &TimelinePdus,
	lazily_loaded_members: Option<&'a MemberSet>,
) -> Result<Vec<PduEvent>> {
	// NB: a limited sync is one where `timeline.limited == true`. Synapse calls
	// this a "gappy" sync internally.

	/*
	the state events returned from an incremental sync which isn't limited are usually empty.
	however, if an event in the timeline (`timeline.pdus`) merges a split in the room's DAG (i.e. has multiple `prev_events`),
	the state at the _end_ of the timeline may include state events which were merged in and don't exist in the state
	at the _start_ of the timeline. because this is uncommon, we check here to see if any events in the timeline
	merged a split in the DAG.

	see: https://github.com/element-hq/synapse/issues/16941
	*/

	let timeline_is_linear = timeline.pdus.is_empty() || {
		let last_pdu_of_last_sync = services
			.rooms
			.timeline
			.pdus_rev(Some(sender_user), room_id, Some(previous_sync_end_count.saturating_add(1)))
			.boxed()
			.next()
			.await
			.transpose()
			.expect("last sync should have had some PDUs")
			.map(at!(1));

		// make sure the prev_events of each pdu in the timeline refer only to the
		// previous pdu
		timeline
			.pdus
			.iter()
			.try_fold(last_pdu_of_last_sync.map(|pdu| pdu.event_id), |prev_event_id, (_, pdu)| {
				if let Ok(pdu_prev_event_id) = pdu.prev_events.iter().exactly_one() {
					if prev_event_id
						.as_ref()
						.is_none_or(is_equal_to!(pdu_prev_event_id))
					{
						return ControlFlow::Continue(Some(pdu_prev_event_id.to_owned()));
					}
				}

				trace!(
					"pdu {:?} has split prev_events (expected {:?}): {:?}",
					pdu.event_id, prev_event_id, pdu.prev_events
				);
				ControlFlow::Break(())
			})
			.is_continue()
	};

	if timeline_is_linear && !timeline.limited {
		// if there are no splits in the DAG and the timeline isn't limited, then
		// `state` will always be empty unless lazy loading is enabled.

		if let Some(lazily_loaded_members) = lazily_loaded_members
			&& !timeline.pdus.is_empty()
		{
			// lazy loading is enabled, so we return the membership events which were
			// requested by the caller.
			let lazy_membership_events: Vec<_> = lazily_loaded_members
				.iter()
				.stream()
				.broad_filter_map(|user_id| async move {
					if user_id == sender_user {
						return None;
					}

					services
						.rooms
						.state_accessor
						.state_get(
							timeline_start_shortstatehash,
							&StateEventType::RoomMember,
							user_id.as_str(),
						)
						.ok()
						.await
				})
				.collect()
				.await;

			if !lazy_membership_events.is_empty() {
				trace!(
					"syncing lazy membership events for members: {:?}",
					lazy_membership_events
						.iter()
						.map(|pdu| pdu.state_key().unwrap())
				);
			}
			return Ok(lazy_membership_events);
		}

		// lazy loading is disabled, `state` is empty.
		return Ok(vec![]);
	}

	/*
	at this point, either the timeline is `limited` or the DAG has a split in it. this necessitates
	computing the incremental state (which may be empty).

	NOTE: this code path does not apply lazy-load filtering to membership state events. the spec forbids lazy-load filtering
	if the timeline is `limited`, and DAG splits which require sending extra membership state events are (probably) uncommon
	enough that the performance penalty is acceptable.
	*/

	trace!(?timeline_is_linear, ?timeline.limited, "computing state for incremental sync");

	// fetch the shorteventids of state events in the timeline
	let state_events_in_timeline: BTreeSet<ShortEventId> = services
		.rooms
		.short
		.multi_get_or_create_shorteventid(timeline.pdus.iter().filter_map(|(_, pdu)| {
			if pdu.state_key().is_some() {
				Some(pdu.event_id.as_ref())
			} else {
				None
			}
		}))
		.collect()
		.await;

	trace!("{} state events in timeline", state_events_in_timeline.len());

	/*
	fetch the state events which were added since the last sync.

	specifically we fetch the difference between the state at the last sync and the state at the _end_
	of the timeline, and then we filter out state events in the timeline itself using the shorteventids we fetched.
	this is necessary to account for splits in the DAG, as explained above.
	*/
	let state_diff = services
		.rooms
		.short
		.multi_get_eventid_from_short::<'_, OwnedEventId, _>(
			services
				.rooms
				.state_accessor
				.state_added((previous_sync_end_shortstatehash, timeline_end_shortstatehash))
				.await?
				.stream()
				.ready_filter_map(|(_, shorteventid)| {
					if state_events_in_timeline.contains(&shorteventid) {
						None
					} else {
						Some(shorteventid)
					}
				}),
		)
		.ignore_err();

	// finally, fetch the PDU contents and collect them into a vec
	let state_diff_pdus = state_diff
		.broad_filter_map(|event_id| async move {
			services
				.rooms
				.timeline
				.get_non_outlier_pdu(&event_id)
				.await
				.ok()
		})
		.collect::<Vec<_>>()
		.await;

	trace!(?state_diff_pdus, "collected state PDUs for incremental sync");
	Ok(state_diff_pdus)
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
