use conduwuit::{
	Event, PduCount, PduEvent, Result, at, debug_warn,
	pdu::EventHash,
	trace,
	utils::{self, IterStream, future::ReadyEqExt, stream::WidebandExt as _},
};
use futures::{StreamExt, future::join};
use ruma::{
	EventId, OwnedRoomId, RoomId,
	api::client::sync::sync_events::v3::{LeftRoom, RoomAccountData, State, Timeline},
	events::{
		StateEventType, TimelineEventType,
		room::member::{MembershipChange, RoomMemberEventContent},
	},
	uint,
};
use serde_json::value::RawValue;
use service::Services;

use crate::client::{
	TimelinePdus, ignored_filter,
	sync::{
		load_timeline,
		v3::{
			DEFAULT_TIMELINE_LIMIT, SyncContext, prepare_lazily_loaded_members,
			state::build_state_initial,
		},
	},
};

#[tracing::instrument(
	name = "left",
	level = "debug",
	skip_all,
	fields(
		room_id = %room_id,
	),
)]
#[allow(clippy::too_many_arguments)]
pub(super) async fn load_left_room(
	services: &Services,
	sync_context: SyncContext<'_>,
	ref room_id: OwnedRoomId,
	leave_pdu: Option<PduEvent>,
) -> Result<Option<LeftRoom>> {
	let SyncContext {
		syncing_user,
		last_sync_end_count,
		current_count,
		filter,
		..
	} = sync_context;

	// the global count as of the moment the user left the room
	let Some(left_count) = services
		.rooms
		.state_cache
		.get_left_count(room_id, syncing_user)
		.await
		.ok()
	else {
		// if we get here, the membership cache is incorrect, likely due to a state
		// reset
		debug_warn!("attempting to sync left room but no left count exists");
		return Ok(None);
	};

	let include_leave = filter.room.include_leave;

	// return early if we haven't gotten to this leave yet.
	// this can happen if the user leaves while a sync response is being generated
	if current_count < left_count {
		return Ok(None);
	}

	// return early if this is an incremental sync, and we've already synced this
	// leave to the user, and `include_leave` isn't set on the filter.
	if !include_leave && last_sync_end_count >= Some(left_count) {
		return Ok(None);
	}

	if let Some(ref leave_pdu) = leave_pdu {
		debug_assert_eq!(
			leave_pdu.kind,
			TimelineEventType::RoomMember,
			"leave PDU should be m.room.member"
		);
	}

	let does_not_exist = services.rooms.metadata.exists(room_id).eq(&false).await;

	let (timeline, state_events) = match leave_pdu {
		| Some(leave_pdu) if does_not_exist => {
			/*
			we have none PDUs with left beef for this room, likely because it was a rejected invite to a room
			which nobody on this homeserver is in. `leave_pdu` is the remote-assisted outlier leave event for the room,
			which is all we can send to the client.
			*/
			trace!("syncing remote-assisted leave PDU");
			(TimelinePdus::default(), vec![leave_pdu])
		},
		| Some(leave_pdu) => {
			// we have this room in our DB, and can fetch the state and timeline from when
			// the user left if they're allowed to see it.

			let leave_state_key = syncing_user;
			debug_assert_eq!(
				Some(leave_state_key.as_str()),
				leave_pdu.state_key(),
				"leave PDU should be for the user requesting the sync"
			);

			let leave_shortstatehash = services
				.rooms
				.state_accessor
				.pdu_shortstatehash(&leave_pdu.event_id)
				.await?;

			let prev_member_event = services
				.rooms
				.state_accessor
				.state_get(
					leave_shortstatehash,
					&StateEventType::RoomMember,
					leave_state_key.as_str(),
				)
				.await?;
			let current_membership: RoomMemberEventContent = leave_pdu.get_content()?;
			let prev_membership: RoomMemberEventContent = prev_member_event.get_content()?;

			match current_membership.membership_change(
				Some(prev_membership.details()),
				&leave_pdu.sender,
				leave_state_key,
			) {
				| MembershipChange::Left => {
					// if the user went from `join` to `leave`, they should be able to view the
					// timeline.

					let timeline_start_count = if let Some(last_sync_end_count) =
						last_sync_end_count
					{
						// for incremental syncs, start the timeline after `since`
						PduCount::Normal(last_sync_end_count)
					} else {
						// for initial syncs, start the timeline at the previous membership event
						services
							.rooms
							.timeline
							.get_pdu_count(&prev_member_event.event_id)
							.await?
							.saturating_sub(1)
					};

					// end the timeline at the user's leave event
					let timeline_end_count = services
						.rooms
						.timeline
						.get_pdu_count(leave_pdu.event_id())
						.await?;

					// limit the timeline using the same logic as for joined rooms
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
						Some(timeline_start_count),
						Some(timeline_end_count),
						timeline_limit,
					)
					.await?;

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

						leave_shortstatehash
					};

					let lazily_loaded_members = prepare_lazily_loaded_members(
						services,
						sync_context,
						room_id,
						timeline.senders(),
					);

					let (timeline_start_shortstatehash, lazily_loaded_members) =
						join(timeline_start_shortstatehash, lazily_loaded_members).await;

					// TODO: calculate incremental state for incremental syncs.
					// always calculating initial state _works_ but returns more data and does
					// more processing than strictly necessary.
					let state = build_state_initial(
						services,
						syncing_user,
						timeline_start_shortstatehash,
						lazily_loaded_members.as_ref(),
					)
					.await?;

					trace!(
						?timeline_start_count,
						?timeline_end_count,
						"syncing {} timeline events (limited = {}) and {} state events",
						timeline.pdus.len(),
						timeline.limited,
						state.len()
					);

					(timeline, state)
				},
				| other_membership => {
					// otherwise, the user should not be able to view the timeline.
					// only return their leave event.
					trace!(
						?other_membership,
						"user did not leave happily, only syncing leave event"
					);
					(TimelinePdus::default(), vec![leave_pdu])
				},
			}
		},
		| None => {
			/*
			no leave event was actually sent in this room, but we still need to pretend
			like the user left it. this is usually because the room was banned by a server admin.
			generate a fake leave event to placate the client.
			*/
			trace!("syncing dummy leave event");
			(TimelinePdus::default(), vec![create_dummy_leave_event(
				services,
				sync_context,
				room_id,
			)])
		},
	};

	let raw_timeline_pdus = timeline
		.pdus
		.into_iter()
		.stream()
		// filter out ignored events from the timeline
		.wide_filter_map(|item| ignored_filter(services, item, syncing_user))
		.map(at!(1))
		.map(Event::into_format)
		.collect::<Vec<_>>()
		.await;

	Ok(Some(LeftRoom {
		account_data: RoomAccountData { events: Vec::new() },
		timeline: Timeline {
			limited: timeline.limited,
			prev_batch: Some(current_count.to_string()),
			events: raw_timeline_pdus,
		},
		state: State {
			events: state_events.into_iter().map(Event::into_format).collect(),
		},
	}))
}

fn create_dummy_leave_event(
	services: &Services,
	SyncContext { syncing_user, .. }: SyncContext<'_>,
	room_id: &RoomId,
) -> PduEvent {
	// TODO: because this event ID is random, it could cause caching issues with
	// clients. perhaps a database table could be created to hold these dummy
	// events, or they could be stored as outliers?
	PduEvent {
		event_id: EventId::new(services.globals.server_name()),
		sender: syncing_user.to_owned(),
		origin: None,
		origin_server_ts: utils::millis_since_unix_epoch()
			.try_into()
			.expect("Timestamp is valid js_int value"),
		kind: TimelineEventType::RoomMember,
		content: RawValue::from_string(r#"{"membership": "leave"}"#.to_owned()).unwrap(),
		state_key: Some(syncing_user.as_str().into()),
		unsigned: None,
		// The following keys are dropped on conversion
		room_id: Some(room_id.to_owned()),
		prev_events: vec![],
		depth: uint!(1),
		auth_events: vec![],
		redacts: None,
		hashes: EventHash { sha256: String::new() },
		signatures: None,
	}
}
