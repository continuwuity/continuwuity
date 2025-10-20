mod v3;
mod v4;
mod v5;

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

pub(crate) use self::{
	v3::sync_events_route, v4::sync_events_v4_route, v5::sync_events_v5_route,
};

pub(crate) const DEFAULT_BUMP_TYPES: &[TimelineEventType; 6] =
	&[CallInvite, PollStart, Beacon, RoomEncrypted, RoomMessage, Sticker];

#[derive(Default)]
pub(crate) struct TimelinePdus {
	pub pdus: Vec<(PduCount, PduEvent)>,
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
	let last_timeline_count = services
		.rooms
		.timeline
		.last_timeline_count(Some(sender_user), room_id)
		.await?;

	let mut pdus_between_counts = match starting_count {
		| Some(starting_count) => {
			if last_timeline_count <= starting_count {
				return Ok(TimelinePdus::default());
			}

			// Stream from the DB all PDUs which were sent after `starting_count` but before
			// `ending_count`, including both endpoints
			services
				.rooms
				.timeline
				.pdus(Some(sender_user), room_id, Some(starting_count))
				.ignore_err()
				.ready_take_while(|&(pducount, _)| {
					pducount <= ending_count.unwrap_or_else(PduCount::max)
				})
				.boxed()
		},
		| None => {
			// For initial sync, stream from the DB all PDUs before and including
			// `ending_count` in reverse order
			services
				.rooms
				.timeline
				.pdus_rev(Some(sender_user), room_id, ending_count)
				.ignore_err()
				.boxed()
		},
	};

	// Return at most `limit` PDUs from the stream
	let mut pdus: Vec<_> = pdus_between_counts.by_ref().take(limit).collect().await;
	if starting_count.is_none() {
		// `pdus_rev` returns PDUs in reverse order. fix that here
		pdus.reverse();
	}
	// The timeline is limited if more than `limit` PDUs exist in the DB after
	// `starting_count`
	let limited = pdus_between_counts.next().await.is_some();

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
