use std::{collections::BTreeSet, ops::ControlFlow};

use conduwuit::{
	Result, at, is_equal_to,
	matrix::{
		Event,
		pdu::{PduCount, PduEvent},
	},
	utils::{
		BoolExt, IterStream, ReadyExt, TryFutureExtExt,
		stream::{BroadbandExt, TryIgnore},
	},
};
use conduwuit_service::{
	Services,
	rooms::{lazy_loading::MemberSet, short::ShortStateHash},
};
use futures::{FutureExt, StreamExt};
use itertools::Itertools;
use ruma::{OwnedEventId, RoomId, UserId, events::StateEventType};
use service::rooms::short::ShortEventId;
use tracing::trace;

use crate::client::TimelinePdus;

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
pub(super) async fn build_state_initial(
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
pub(super) async fn build_state_incremental<'a>(
	services: &Services,
	sender_user: &'a UserId,
	room_id: &RoomId,
	last_sync_end_count: PduCount,
	last_sync_end_shortstatehash: ShortStateHash,
	timeline_start_shortstatehash: ShortStateHash,
	timeline_end_shortstatehash: ShortStateHash,
	timeline: &TimelinePdus,
	lazily_loaded_members: Option<&'a MemberSet>,
) -> Result<Vec<PduEvent>> {
	/*
	NB: a limited sync is one where `timeline.limited == true`. Synapse calls this a "gappy" sync internally.

	The algorithm implemented in this function is, currently, quite different from the algorithm vaguely described
	by the Matrix specification. This is because the specification's description of the `state` property does not accurately
	reflect how Synapse behaves, and therefore how client SDKs behave. Notable differences include:
	1. We do not compute the delta using the naive approach of "every state event from the end of the last sync
	   up to the start of this sync's timeline". see below for details.
	2. If lazy-loading is enabled, we include lazily-loaded membership events. The specific users to include are determined
	   elsewhere and supplied to this function in the `lazily_loaded_members` parameter.
	*/

	/*
	the `state` property of an incremental sync which isn't limited are _usually_ empty.
	(note: the specification says that the `state` property is _always_ empty for limited syncs, which is incorrect.)
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
			.pdus_rev(Some(sender_user), room_id, Some(last_sync_end_count.saturating_add(1)))
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

		if let Some(lazily_loaded_members) = lazily_loaded_members {
			if !timeline.pdus.is_empty() {
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
							.collect::<Vec<_>>()
					);
				}
				return Ok(lazy_membership_events);
			}
		}

		// lazy loading is disabled, `state` is empty.
		return Ok(vec![]);
	}

	/*
	at this point, either the timeline is `limited` or the DAG has a split in it. this necessitates
	computing the incremental state (which may be empty).

	NOTE: this code path does not use the `lazy_membership_events` parameter. any changes to membership will be included
	in the incremental state. therefore, the incremental state may include "redundant" membership events,
	which we do not filter out because A. the spec forbids lazy-load filtering if the timeline is `limited`,
	and B. DAG splits which require sending extra membership state events are (probably) uncommon enough that
	the performance penalty is acceptable.
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
				.state_added((last_sync_end_shortstatehash, timeline_end_shortstatehash))
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
