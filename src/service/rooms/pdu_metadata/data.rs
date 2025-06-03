use std::{mem::size_of, sync::Arc};

use conduwuit::{
	arrayvec::ArrayVec,
	matrix::{Event, PduCount},
	utils::{
		ReadyExt,
		stream::{TryIgnore, WidebandExt},
		u64_from_u8,
	},
};
use database::Map;
use futures::{Stream, StreamExt};
use ruma::{EventId, RoomId, UserId, api::Direction};

use crate::{
	Dep, rooms,
	rooms::{
		short::{ShortEventId, ShortRoomId},
		timeline::{PduId, RawPduId},
	},
};

pub(super) struct Data {
	tofrom_relation: Arc<Map>,
	referencedevents: Arc<Map>,
	softfailedeventids: Arc<Map>,
	services: Services,
}

struct Services {
	timeline: Dep<rooms::timeline::Service>,
}

impl Data {
	pub(super) fn new(args: &crate::Args<'_>) -> Self {
		let db = &args.db;
		Self {
			tofrom_relation: db["tofrom_relation"].clone(),
			referencedevents: db["referencedevents"].clone(),
			softfailedeventids: db["softfailedeventids"].clone(),
			services: Services {
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
			},
		}
	}

	pub(super) fn add_relation(&self, from: u64, to: u64) {
		const BUFSIZE: usize = size_of::<u64>() * 2;

		let key: &[u64] = &[to, from];
		self.tofrom_relation.aput_raw::<BUFSIZE, _, _>(key, []);
	}

	pub(super) fn get_relations<'a>(
		&'a self,
		user_id: &'a UserId,
		shortroomid: ShortRoomId,
		target: ShortEventId,
		from: PduCount,
		dir: Direction,
	) -> impl Stream<Item = (PduCount, impl Event)> + Send + 'a {
		// Query from exact position then filter excludes it (saturating_inc could skip
		// events at min/max boundaries)
		let from_unsigned = from.into_unsigned();
		let mut current = ArrayVec::<u8, 16>::new();
		current.extend(target.to_be_bytes());
		current.extend(from_unsigned.to_be_bytes());
		let current = current.as_slice();
		match dir {
			| Direction::Forward => self.tofrom_relation.raw_keys_from(current).boxed(),
			| Direction::Backward => self.tofrom_relation.rev_raw_keys_from(current).boxed(),
		}
		.ignore_err()
		.ready_take_while(move |key| key.starts_with(&target.to_be_bytes()))
		.map(|to_from| u64_from_u8(&to_from[8..16]))
		.map(PduCount::from_unsigned)
		.ready_filter(move |count| {
			if from == PduCount::min() || from == PduCount::max() {
				true
			} else {
				let count_unsigned = count.into_unsigned();
				match dir {
					| Direction::Forward => count_unsigned > from_unsigned,
					| Direction::Backward => count_unsigned < from_unsigned,
				}
			}
		})
		.wide_filter_map(move |shorteventid| async move {
			let pdu_id: RawPduId = PduId { shortroomid, shorteventid }.into();

			let mut pdu = self.services.timeline.get_pdu_from_id(&pdu_id).await.ok()?;

			pdu.as_mut_pdu().set_unsigned(Some(user_id));

			Some((shorteventid, pdu))
		})
	}

	#[inline]
	pub(super) fn mark_as_referenced<'a, I>(&self, room_id: &RoomId, event_ids: I)
	where
		I: Iterator<Item = &'a EventId>,
	{
		for prev in event_ids {
			let key = (room_id, prev);
			self.referencedevents.put_raw(key, []);
		}
	}

	pub(super) async fn is_event_referenced(&self, room_id: &RoomId, event_id: &EventId) -> bool {
		let key = (room_id, event_id);
		self.referencedevents.qry(&key).await.is_ok()
	}

	pub(super) fn mark_event_soft_failed(&self, event_id: &EventId) {
		self.softfailedeventids.insert(event_id, []);
	}

	pub(super) async fn is_event_soft_failed(&self, event_id: &EventId) -> bool {
		self.softfailedeventids.get(event_id).await.is_ok()
	}
}
