mod v3;
mod v4;
mod v5;

use conduwuit::{
	Error, PduCount, Result,
	matrix::pdu::PduEvent,
	utils::stream::{BroadbandExt, ReadyExt, TryIgnore},
};
use conduwuit_service::Services;
use futures::{StreamExt, pin_mut};
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

async fn load_timeline(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	roomsincecount: PduCount,
	next_batch: Option<PduCount>,
	limit: usize,
) -> Result<(Vec<(PduCount, PduEvent)>, bool), Error> {
	let last_timeline_count = services
		.rooms
		.timeline
		.last_timeline_count(Some(sender_user), room_id)
		.await?;

	if last_timeline_count <= roomsincecount {
		return Ok((Vec::new(), false));
	}

	let non_timeline_pdus = services
		.rooms
		.timeline
		.pdus_rev(Some(sender_user), room_id, None)
		.ignore_err()
		.ready_skip_while(|&(pducount, _)| pducount > next_batch.unwrap_or_else(PduCount::max))
		.ready_take_while(|&(pducount, _)| pducount > roomsincecount);

	// Take the last events for the timeline
	pin_mut!(non_timeline_pdus);
	let timeline_pdus: Vec<_> = non_timeline_pdus.by_ref().take(limit).collect().await;

	let timeline_pdus: Vec<_> = timeline_pdus.into_iter().rev().collect();

	// They /sync response doesn't always return all messages, so we say the output
	// is limited unless there are events in non_timeline_pdus
	let limited = non_timeline_pdus.next().await.is_some();

	Ok((timeline_pdus, limited))
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
