use std::collections::{BTreeMap, HashSet};

use conduwuit::{
	Result, at, debug_warn, err, extract_variant,
	matrix::{
		Event,
		pdu::{PduCount, PduEvent},
	},
	trace,
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
	future::{OptionFuture, join, join3, join4, try_join, try_join3},
};
use ruma::{
	OwnedRoomId, OwnedUserId, RoomId, UserId,
	api::client::sync::sync_events::{
		UnreadNotificationsCount,
		v3::{Ephemeral, JoinedRoom, RoomAccountData, RoomSummary, State as RoomState, Timeline},
	},
	events::{
		AnyRawAccountDataEvent, StateEventType,
		TimelineEventType::*,
		room::member::{MembershipState, RoomMemberEventContent},
	},
	serde::Raw,
	uint,
};
use service::rooms::short::ShortStateHash;

use super::{load_timeline, share_encrypted_room};
use crate::client::{
	TimelinePdus, ignored_filter,
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
		room_id = %room_id,
		syncing_user = %sync_context.syncing_user,
	),
)]
pub(super) async fn load_joined_room(
	services: &Services,
	sync_context: SyncContext<'_>,
	ref room_id: OwnedRoomId,
) -> Result<(JoinedRoom, DeviceListUpdates)> {
	/*
	Building a sync response involves many steps which all depend on each other.
	To parallelize the process as much as possible, each step is divided into its own function,
	and `join*` functions are used to perform steps in parallel which do not depend on each other.
	*/

	let insert_lock = services.rooms.timeline.mutex_insert.lock(room_id).await;
	#[cfg(feature = "sync-drop-instant")]
	drop(insert_lock);
	let (
		account_data,
		ephemeral,
		StateAndTimeline {
			state_events,
			timeline,
			summary,
			notification_counts,
			device_list_updates,
		},
	) = try_join3(
		build_account_data(services, sync_context, room_id),
		build_ephemeral(services, sync_context, room_id),
		build_state_and_timeline(services, sync_context, room_id),
	)
	.boxed()
	.await?;
	#[cfg(not(feature = "sync-drop-instant"))]
	drop(insert_lock);

	if !timeline.is_empty() || !state_events.is_empty() {
		trace!(
			"syncing {} timeline events (limited = {}) and {} state events",
			timeline.events.len(),
			timeline.limited,
			state_events.len()
		);
	}

	let joined_room = JoinedRoom {
		account_data,
		summary: summary.unwrap_or_default(),
		unread_notifications: notification_counts.unwrap_or_default(),
		timeline,
		state: RoomState {
			events: state_events.into_iter().map(Event::into_format).collect(),
		},
		ephemeral,
		unread_thread_notifications: BTreeMap::new(),
	};

	Ok((joined_room, device_list_updates))
}

/// Collect changes to the syncing user's account data events.
#[tracing::instrument(level = "debug", skip_all)]
async fn build_account_data(
	services: &Services,
	SyncContext {
		syncing_user,
		last_sync_end_count,
		current_count,
		..
	}: SyncContext<'_>,
	room_id: &RoomId,
) -> Result<RoomAccountData> {
	let account_data_changes = services
		.account_data
		.changes_since(Some(room_id), syncing_user, last_sync_end_count, Some(current_count))
		.ready_filter_map(|e| extract_variant!(e, AnyRawAccountDataEvent::Room))
		.collect()
		.await;

	Ok(RoomAccountData { events: account_data_changes })
}

/// Collect new ephemeral events.
#[tracing::instrument(level = "debug", skip_all)]
async fn build_ephemeral(
	services: &Services,
	SyncContext { syncing_user, last_sync_end_count, .. }: SyncContext<'_>,
	room_id: &RoomId,
) -> Result<Ephemeral> {
	// note: some of the futures below are boxed. this is because, without the box,
	// rustc produces over thirty inscrutable errors in `mod.rs` at the call-site
	// of `load_joined_room`. I don't know why boxing them fixes this -- it seems
	// to be related to the async closures and borrowing from the sync context.

	// collect updates to read receipts
	let receipt_events = services
		.rooms
		.read_receipt
		.readreceipts_since(room_id, last_sync_end_count)
		.filter_map(async |(read_user, _, edu)| {
			let is_ignored = services
				.users
				.user_is_ignored(&read_user, syncing_user)
				.await;

			// filter out read receipts for ignored users
			is_ignored.or_some(edu)
		})
		.collect::<Vec<_>>()
		.boxed();

	// collect the updated list of typing users, if it's changed
	let typing_event = async {
		let should_send_typing_event = match last_sync_end_count {
			| Some(last_sync_end_count) => {
				match services.rooms.typing.last_typing_update(room_id).await {
					| Ok(last_typing_update) => {
						// update the typing list if the users typing have changed since the last
						// sync
						last_typing_update > last_sync_end_count
					},
					| Err(err) => {
						warn!("Error checking last typing update: {}", err);
						return None;
					},
				}
			},
			// always update the typing list on an initial sync
			| None => true,
		};

		if should_send_typing_event {
			let event = services
				.rooms
				.typing
				.typings_event_for_user(room_id, syncing_user)
				.await;

			if let Ok(event) = event {
				return Some(
					Raw::new(&event)
						.expect("typing event should be valid")
						.cast(),
				);
			}
		}

		None
	};

	// collect the syncing user's private-read marker, if it's changed
	let private_read_event = async {
		let should_send_private_read = match last_sync_end_count {
			| Some(last_sync_end_count) => {
				let last_privateread_update = services
					.rooms
					.read_receipt
					.last_privateread_update(syncing_user, room_id)
					.await;

				// update the marker if it's changed since the last sync
				last_privateread_update > last_sync_end_count
			},
			// always update the marker on an initial sync
			| None => true,
		};

		if should_send_private_read {
			services
				.rooms
				.read_receipt
				.private_read_get(room_id, syncing_user)
				.await
				.ok()
		} else {
			None
		}
	};

	let (receipt_events, typing_event, private_read_event) =
		join3(receipt_events, typing_event, private_read_event).await;

	let mut edus = receipt_events;
	edus.extend(typing_event);
	edus.extend(private_read_event);

	Ok(Ephemeral { events: edus })
}

/// A struct to hold the state events, timeline, and other data which is
/// computed from them.
struct StateAndTimeline {
	state_events: Vec<PduEvent>,
	timeline: Timeline,
	summary: Option<RoomSummary>,
	notification_counts: Option<UnreadNotificationsCount>,
	device_list_updates: DeviceListUpdates,
}

/// Compute changes to the room's state and timeline.
#[tracing::instrument(level = "debug", skip_all)]
async fn build_state_and_timeline(
	services: &Services,
	sync_context: SyncContext<'_>,
	room_id: &RoomId,
) -> Result<StateAndTimeline> {
	let (shortstatehashes, timeline) = try_join(
		fetch_shortstatehashes(services, sync_context, room_id),
		build_timeline(services, sync_context, room_id),
	)
	.await?;

	let (state_events, notification_counts, joined_since_last_sync) = try_join3(
		build_state_events(services, sync_context, room_id, shortstatehashes, &timeline),
		build_notification_counts(services, sync_context, room_id, &timeline),
		check_joined_since_last_sync(services, shortstatehashes, sync_context),
	)
	.await?;

	// the timeline should always include at least one PDU if the syncing user
	// joined since the last sync, that being the syncing user's join event. if
	// it's empty something is wrong.
	if joined_since_last_sync && timeline.pdus.is_empty() {
		trace!("timeline for newly joined room is empty");
	}

	let (summary, device_list_updates) = try_join(
		build_room_summary(
			services,
			sync_context,
			room_id,
			shortstatehashes,
			&timeline,
			&state_events,
			joined_since_last_sync,
		),
		build_device_list_updates(
			services,
			sync_context,
			room_id,
			shortstatehashes,
			&state_events,
			joined_since_last_sync,
		),
	)
	.await?;

	// the token which may be passed to the messages endpoint to backfill room
	// history
	let prev_batch = timeline.pdus.front().map(at!(0));

	// note: we always indicate a limited timeline if the syncing user just joined
	// the room, to indicate to the client that it should request backfill (and to
	// copy Synapse's behavior). for federated room joins, the `timeline` will
	// usually only include the syncing user's join event.
	let limited = timeline.limited || joined_since_last_sync;

	// filter out ignored events from the timeline and convert the PDUs into Ruma's
	// AnySyncTimelineEvent type
	let filtered_timeline = timeline
		.pdus
		.into_iter()
		.stream()
		.wide_filter_map(|item| ignored_filter(services, item, sync_context.syncing_user))
		.map(at!(1))
		.map(Event::into_format)
		.collect::<Vec<_>>()
		.await;

	Ok(StateAndTimeline {
		state_events,
		timeline: Timeline {
			limited,
			prev_batch: prev_batch.as_ref().map(ToString::to_string),
			events: filtered_timeline,
		},
		summary,
		notification_counts,
		device_list_updates,
	})
}

/// Shortstatehashes necessary to compute what state events to sync.
#[derive(Clone, Copy)]
struct ShortStateHashes {
	/// The current state of the syncing room.
	current_shortstatehash: ShortStateHash,
	/// The state of the syncing room at the end of the last sync.
	last_sync_end_shortstatehash: Option<ShortStateHash>,
}

/// Fetch the current_shortstatehash and last_sync_end_shortstatehash.
#[tracing::instrument(level = "debug", skip_all)]
async fn fetch_shortstatehashes(
	services: &Services,
	SyncContext { last_sync_end_count, current_count, .. }: SyncContext<'_>,
	room_id: &RoomId,
) -> Result<ShortStateHashes> {
	// the room state currently.
	// TODO: this should be the room state as of `current_count`, but there's no way
	// to get that right now.
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
			// look up the shortstatehash saved by the last sync's call to
			// `associate_token_shortstatehash`
			services
				.rooms
				.user
				.get_token_shortstatehash(room_id, last_sync_end_count)
				.inspect_err(move |_| {
					debug_warn!(
						token = last_sync_end_count,
						"Room has no shortstatehash for this token"
					);
				})
				.ok()
		}))
		.map(Option::flatten)
		.map(Ok);

	let (current_shortstatehash, last_sync_end_shortstatehash) =
		try_join(current_shortstatehash, last_sync_end_shortstatehash).await?;

	/*
	associate the `current_count` with the `current_shortstatehash`, so we can
	use it on the next sync as the `last_sync_end_shortstatehash`.

	TODO: the table written to by this call grows extremely fast, gaining one new entry for each
	joined room on _every single sync request_. we need to find a better way to remember the shortstatehash
	between syncs.
	*/
	services
		.rooms
		.user
		.associate_token_shortstatehash(room_id, current_count, current_shortstatehash)
		.await;

	Ok(ShortStateHashes {
		current_shortstatehash,
		last_sync_end_shortstatehash,
	})
}

/// Fetch recent timeline events.
#[tracing::instrument(level = "debug", skip_all)]
async fn build_timeline(
	services: &Services,
	sync_context: SyncContext<'_>,
	room_id: &RoomId,
) -> Result<TimelinePdus> {
	let SyncContext {
		syncing_user,
		last_sync_end_count,
		current_count,
		filter,
		..
	} = sync_context;

	/*
	determine the maximum number of events to return in this sync.
	if the sync filter specifies a limit, that will be used, otherwise
	`DEFAULT_TIMELINE_LIMIT` will be used. `DEFAULT_TIMELINE_LIMIT` will also be
	used if the limit is somehow greater than usize::MAX.
	*/
	let timeline_limit = filter
		.room
		.timeline
		.limit
		.and_then(|limit| limit.try_into().ok())
		.unwrap_or(DEFAULT_TIMELINE_LIMIT);

	load_timeline(
		services,
		syncing_user,
		room_id,
		last_sync_end_count.map(PduCount::Normal),
		Some(PduCount::Normal(current_count)),
		timeline_limit,
	)
	.await
}

/// Calculate the state events to sync.
async fn build_state_events(
	services: &Services,
	sync_context: SyncContext<'_>,
	room_id: &RoomId,
	shortstatehashes: ShortStateHashes,
	timeline: &TimelinePdus,
) -> Result<Vec<PduEvent>> {
	let SyncContext {
		syncing_user,
		last_sync_end_count,
		full_state,
		..
	} = sync_context;

	let ShortStateHashes {
		current_shortstatehash,
		last_sync_end_shortstatehash,
	} = shortstatehashes;

	// the spec states that the `state` property only includes state events up to
	// the beginning of the timeline, so we determine the state of the syncing room
	// as of the first timeline event. NOTE: this explanation is not entirely
	// accurate; see the implementation of `build_state_incremental`.
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

	// the user IDs of members whose membership needs to be sent to the client, if
	// lazy-loading is enabled.
	let lazily_loaded_members =
		prepare_lazily_loaded_members(services, sync_context, room_id, timeline.senders());

	let (timeline_start_shortstatehash, lazily_loaded_members) =
		join(timeline_start_shortstatehash, lazily_loaded_members).await;

	// compute the state delta between the previous sync and this sync.
	match (last_sync_end_count, last_sync_end_shortstatehash) {
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
				timeline,
				lazily_loaded_members.as_ref(),
			)
			.boxed()
			.await,
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
			.await,
	}
}

/// Compute the number of unread notifications in this room.
#[tracing::instrument(level = "debug", skip_all)]
async fn build_notification_counts(
	services: &Services,
	SyncContext { syncing_user, last_sync_end_count, .. }: SyncContext<'_>,
	room_id: &RoomId,
	timeline: &TimelinePdus,
) -> Result<Option<UnreadNotificationsCount>> {
	// determine whether to actually update the notification counts
	let should_send_notification_counts = async {
		// if we're going to sync some timeline events, the notification count has
		// definitely changed to include them
		if !timeline.pdus.is_empty() {
			return true;
		}

		// if this is an initial sync, we need to send notification counts because the
		// client doesn't know what they are yet
		let Some(last_sync_end_count) = last_sync_end_count else {
			return true;
		};

		let last_notification_read = services
			.rooms
			.user
			.last_notification_read(syncing_user, room_id)
			.await;

		// if the syncing user has read the events we sent during the last sync, we need
		// to send a new notification count on this sync.
		if last_notification_read > last_sync_end_count {
			return true;
		}

		// otherwise, nothing's changed.
		false
	};

	if should_send_notification_counts.await {
		let (notification_count, highlight_count) = join(
			services
				.rooms
				.user
				.notification_count(syncing_user, room_id)
				.map(TryInto::try_into)
				.unwrap_or(uint!(0)),
			services
				.rooms
				.user
				.highlight_count(syncing_user, room_id)
				.map(TryInto::try_into)
				.unwrap_or(uint!(0)),
		)
		.await;

		trace!(%notification_count, %highlight_count, "syncing new notification counts");

		Ok(Some(UnreadNotificationsCount {
			notification_count: Some(notification_count),
			highlight_count: Some(highlight_count),
		}))
	} else {
		Ok(None)
	}
}

/// Check if the syncing user joined the room since their last incremental sync.
#[tracing::instrument(level = "debug", skip_all)]
async fn check_joined_since_last_sync(
	services: &Services,
	ShortStateHashes { last_sync_end_shortstatehash, .. }: ShortStateHashes,
	SyncContext { syncing_user, .. }: SyncContext<'_>,
) -> Result<bool> {
	// fetch the syncing user's membership event during the last sync.
	// this will be None if `previous_sync_end_shortstatehash` is None.
	let membership_during_previous_sync = match last_sync_end_shortstatehash {
		| Some(last_sync_end_shortstatehash) => services
			.rooms
			.state_accessor
			.state_get_content(
				last_sync_end_shortstatehash,
				&StateEventType::RoomMember,
				syncing_user.as_str(),
			)
			.await
			.inspect_err(|_| debug_warn!("User has no previous membership"))
			.ok(),
		| None => None,
	};

	// TODO: If the requesting user got state-reset out of the room, this
	// will be `true` when it shouldn't be. this function should never be called
	// in that situation, but it may be if the membership cache didn't get updated.
	// the root cause of this needs to be addressed
	let joined_since_last_sync =
		membership_during_previous_sync.is_none_or(|content: RoomMemberEventContent| {
			content.membership != MembershipState::Join
		});

	if joined_since_last_sync {
		trace!("user joined since last sync");
	}

	Ok(joined_since_last_sync)
}

/// Build the `summary` field of the room object, which includes
/// the number of joined and invited users and the room's heroes.
#[tracing::instrument(level = "debug", skip_all)]
async fn build_room_summary(
	services: &Services,
	SyncContext { syncing_user, .. }: SyncContext<'_>,
	room_id: &RoomId,
	ShortStateHashes { current_shortstatehash, .. }: ShortStateHashes,
	timeline: &TimelinePdus,
	state_events: &[PduEvent],
	joined_since_last_sync: bool,
) -> Result<Option<RoomSummary>> {
	// determine whether any events in the state or timeline are membership events.
	let are_syncing_membership_events = timeline
		.pdus
		.iter()
		.map(|(_, pdu)| pdu)
		.chain(state_events.iter())
		.any(|event| event.kind == RoomMember);

	/*
	we only need to send an updated room summary if:
	1. there are membership events in the state or timeline, because they might have changed the
	   membership counts or heroes, or
	2. the syncing user just joined this room, which usually implies #1 because their join event should be in the timeline.
	*/
	if !(are_syncing_membership_events || joined_since_last_sync) {
		return Ok(None);
	}

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
	let heroes = if !(has_name || has_canonical_alias) {
		Some(build_heroes(services, room_id, syncing_user, current_shortstatehash).await)
	} else {
		None
	};

	trace!(
		%joined_member_count,
		%invited_member_count,
		heroes_length = heroes.as_ref().map(HashSet::len),
		"syncing updated summary"
	);

	Ok(Some(RoomSummary {
		heroes: heroes
			.map(|heroes| heroes.into_iter().collect())
			.unwrap_or_default(),
		joined_member_count: Some(ruma_from_u64(joined_member_count)),
		invited_member_count: Some(ruma_from_u64(invited_member_count)),
	}))
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

/// Collect updates to users' device lists for E2EE.
#[tracing::instrument(level = "debug", skip_all)]
async fn build_device_list_updates(
	services: &Services,
	SyncContext {
		syncing_user,
		last_sync_end_count,
		current_count,
		..
	}: SyncContext<'_>,
	room_id: &RoomId,
	ShortStateHashes { current_shortstatehash, .. }: ShortStateHashes,
	state_events: &Vec<PduEvent>,
	joined_since_last_sync: bool,
) -> Result<DeviceListUpdates> {
	let is_encrypted_room = services
		.rooms
		.state_accessor
		.state_get(current_shortstatehash, &StateEventType::RoomEncryption, "")
		.is_ok();

	// initial syncs don't include device updates, and rooms which aren't encrypted
	// don't affect them, so return early in either of those cases
	if last_sync_end_count.is_none() || !(is_encrypted_room.await) {
		return Ok(DeviceListUpdates::new());
	}

	let mut device_list_updates = DeviceListUpdates::new();

	// add users with changed keys to the `changed` list
	services
		.users
		.room_keys_changed(room_id, last_sync_end_count, Some(current_count))
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

	if !device_list_updates.is_empty() {
		trace!(
			changed = device_list_updates.changed.len(),
			left = device_list_updates.left.len(),
			"syncing device list updates"
		);
	}

	Ok(device_list_updates)
}
