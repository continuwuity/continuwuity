use std::collections::HashMap;

use conduwuit::{
	Event, PduEvent, Result, error,
	pdu::EventHash,
	utils::{self, FutureBoolExt, TryFutureExtExt, future::ReadyEqExt},
	warn,
};
use futures::{FutureExt, StreamExt, pin_mut};
use ruma::{
	EventId, OwnedEventId, OwnedRoomId, UserId,
	api::client::sync::sync_events::v3::{LeftRoom, RoomAccountData, State, Timeline},
	events::{StateEventType, TimelineEventType::*},
	uint,
};
use service::{Services, rooms::lazy_loading::Options};

use crate::client::sync::v3::SyncContext;

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
pub(super) async fn load_left_room(
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
			state: State { events: vec![event.into_format()] },
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
		state: State { events: left_state_events },
	}))
}
