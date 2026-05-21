use std::collections::HashSet;

use conduwuit::{
	Result, at,
	matrix::{Event, pdu::PduEvent},
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
use ruma::{OwnedEventId, UserId, events::StateEventType};
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
	timeline_end_shortstatehash: ShortStateHash,
	use_state_after: bool,
	lazily_loaded_members: Option<&MemberSet>,
) -> Result<Vec<PduEvent>> {
	// load the keys and event IDs of the state events at the start of the timeline
	let (shortstatekeys, event_ids): (Vec<_>, Vec<_>) = services
		.rooms
		.state_accessor
		.state_full_ids(if use_state_after {
			timeline_end_shortstatehash
		} else {
			timeline_start_shortstatehash
		})
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
	last_sync_end_shortstatehash: ShortStateHash,
	timeline_start_shortstatehash: ShortStateHash,
	timeline_end_shortstatehash: ShortStateHash,
	timeline: &TimelinePdus,
	use_state_after: bool,
	lazily_loaded_members: Option<&'a MemberSet>,
) -> Result<Vec<PduEvent>> {
	let mut state_event_ids: HashSet<OwnedEventId> = HashSet::new();

	trace!(
		%use_state_after,
		%last_sync_end_shortstatehash,
		%timeline_start_shortstatehash,
		%timeline_end_shortstatehash,
		"computing state for incremental sync"
	);

	// Fetch lazy-loaded membership events if lazy-loading is enabled
	if let Some(lazily_loaded_members) = lazily_loaded_members
		&& !lazily_loaded_members.is_empty()
	{
		trace!("including lazy membership events for members: {:?}", lazily_loaded_members);

		services
			.rooms
			.short
			.multi_get_eventid_from_short::<'_, OwnedEventId, _>(
				lazily_loaded_members
					.iter()
					.stream()
					.broad_filter_map(|user_id| async move {
						if user_id == sender_user {
							return None;
						}

						services
							.rooms
							.state_accessor
							.state_get_shortid(
								timeline_start_shortstatehash,
								&StateEventType::RoomMember,
								user_id.as_str(),
							)
							.ok()
							.await
					}),
			)
			.ignore_err()
			.ready_for_each(|event_id| {
				state_event_ids.insert(event_id);
			})
			.await;
	}

	// Fetch the state events added since the last sync.
	services
		.rooms
		.short
		.multi_get_eventid_from_short::<'_, OwnedEventId, _>(
			services
				.rooms
				.state_accessor
				.state_added((last_sync_end_shortstatehash, timeline_end_shortstatehash))
				.await?
				.stream()
				.map(at!(1)),
		)
		.ignore_err()
		.ready_for_each(|event_id| {
			state_event_ids.insert(event_id);
		})
		.await;

	if !use_state_after {
		// If state_after isn't enabled, filter out state events which also exist
		// in the timeline. If splits exist in the DAG, this may not be exactly the same
		// thing as the state diff ending at the start of the timeline, but Synapse
		// also does this and it's technically more useful behavior anyway.
		// See: https://github.com/element-hq/synapse/issues/16941

		for (_, pdu) in &timeline.pdus {
			state_event_ids.remove(pdu.event_id());
		}
	}

	// Finally, fetch the PDU contents and collect them into a vec
	let state_diff_pdus = state_event_ids
		.stream()
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
