mod v3;
mod v5;

use std::collections::VecDeque;

use conduwuit::{
	PduCount, Result,
	matrix::pdu::PduEvent,
	trace,
	utils::stream::{BroadbandExt, ReadyExt, TryIgnore},
};
use conduwuit_service::Services;
use futures::StreamExt;
use ruma::{
	RoomId, UserId,
	events::TimelineEventType::{
		self, Beacon, CallInvite, PollStart, RoomEncrypted, RoomMessage, Sticker,
	},
};

pub(crate) use self::{v3::sync_events_route, v5::sync_events_v5_route};

pub(crate) const DEFAULT_BUMP_TYPES: &[TimelineEventType; 6] =
	&[CallInvite, PollStart, Beacon, RoomEncrypted, RoomMessage, Sticker];

#[derive(Default)]
pub(crate) struct TimelinePdus {
	pub pdus: VecDeque<(PduCount, PduEvent)>,
	pub limited: bool,
}

async fn load_timeline(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	starting_count: Option<PduCount>,
	ending_count: Option<PduCount>,
	limit: usize,
) -> Result<TimelinePdus> {
	let mut pdu_stream = match starting_count {
		| Some(starting_count) => {
			let last_timeline_count = services
				.rooms
				.timeline
				.last_timeline_count(Some(sender_user), room_id)
				.await?;

			if last_timeline_count <= starting_count {
				// no messages have been sent in this room since `starting_count`
				return Ok(TimelinePdus::default());
			}

			// for incremental sync, stream from the DB all PDUs which were sent after
			// `starting_count` but before `ending_count`, including `ending_count` but
			// not `starting_count`. this code is pretty similar to the initial sync
			// branch, they're separate to allow for future optimization
			services
				.rooms
				.timeline
				.pdus_rev(
					Some(sender_user),
					room_id,
					ending_count.map(|count| count.saturating_add(1)),
				)
				.ignore_err()
				.ready_take_while(move |&(pducount, _)| pducount > starting_count)
				.boxed()
		},
		| None => {
			// For initial sync, stream from the DB all PDUs before and including
			// `ending_count` in reverse order
			services
				.rooms
				.timeline
				.pdus_rev(
					Some(sender_user),
					room_id,
					ending_count.map(|count| count.saturating_add(1)),
				)
				.ignore_err()
				.boxed()
		},
	};

	// Return at most `limit` PDUs from the stream
	let pdus = pdu_stream
		.by_ref()
		.take(limit)
		.ready_fold(VecDeque::with_capacity(limit), |mut pdus, item| {
			pdus.push_front(item);
			pdus
		})
		.await;

	// The timeline is limited if there are still more PDUs in the stream
	let limited = pdu_stream.next().await.is_some();

	trace!(
		"syncing {:?} timeline pdus from {:?} to {:?} (limited = {:?})",
		pdus.len(),
		starting_count,
		ending_count,
		limited,
	);

	Ok(TimelinePdus { pdus, limited })
}

async fn share_encrypted_room(
	services: &Services,
	sender_user: &UserId,
	user_id: &UserId,
	ignore_room: Option<&RoomId>,
) -> bool {
	services
		.rooms
		.state_cache
		.get_shared_rooms(sender_user, user_id)
		.ready_filter(|&room_id| Some(room_id) != ignore_room)
		.map(ToOwned::to_owned)
		.broad_any(|other_room_id| async move {
			services
				.rooms
				.state_accessor
				.is_encrypted_room(&other_room_id)
				.await
		})
		.await
}
