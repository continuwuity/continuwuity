use std::{mem::size_of, sync::Arc};

use conduwuit::{
	arrayvec::ArrayVec,
	matrix::{PduCount, PduEvent},
	utils::{
		ReadyExt,
		stream::{TryIgnore, WidebandExt},
		u64_from_u8,
	},
};
use database::Map;
use futures::{Stream, StreamExt};
use ruma::{EventId, api::Direction};

use crate::{
	Dep,
	rooms::{
		self,
		short::ShortRoomId,
		timeline::{PduId, RawPduId},
	},
};

pub(super) struct Data {
	tofrom_relation: Arc<Map>,
	softfailedeventids: Arc<Map>,
	rejectedeventids: Arc<Map>,
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
			softfailedeventids: db["softfailedeventids"].clone(),
			rejectedeventids: db["rejectedeventids"].clone(),
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
		shortroomid: ShortRoomId,
		target: u64,
		from: PduCount,
		dir: Direction,
	) -> impl Stream<Item = (u64, PduEvent)> + Send + 'a {
		let start_count = match from {
			| PduCount::Normal(count) => count,
			| PduCount::Backfilled(_) => 0,
		};
		let mut start = ArrayVec::<u8, 16>::new();
		start.extend(target.to_be_bytes());
		start.extend(start_count.to_be_bytes());
		let start = start.as_slice();
		match dir {
			| Direction::Forward => self.tofrom_relation.raw_keys_from(start).boxed(),
			| Direction::Backward => self.tofrom_relation.rev_raw_keys_from(start).boxed(),
		}
		.ignore_err()
		.ready_take_while(move |key| key.starts_with(&target.to_be_bytes()))
		.map(|to_from| u64_from_u8(&to_from[8..16]))
		// The scan includes `from` itself, which should be skipped
		.ready_filter(move |&count| PduCount::Normal(count).is_after(from, dir))
		.wide_filter_map(move |count| async move {
			let shorteventid = PduCount::Normal(count);
			let pdu_id: RawPduId = PduId { shortroomid, shorteventid }.into();
			let pdu = self.services.timeline.get_pdu_from_id(&pdu_id).await.ok()?;

			Some((count, pdu))
		})
	}

	pub(super) fn mark_event_soft_failed(&self, event_id: &EventId) {
		self.softfailedeventids.insert(event_id, []);
	}

	pub(super) fn unmark_event_soft_failed(&self, event_id: &EventId) {
		self.softfailedeventids.remove(event_id);
	}

	pub(super) async fn is_event_soft_failed(&self, event_id: &EventId) -> bool {
		self.softfailedeventids.get(event_id).await.is_ok()
	}

	pub(super) fn mark_event_rejected(&self, event_id: &EventId) {
		self.rejectedeventids.insert(event_id, []);
	}

	pub(super) fn unmark_event_rejected(&self, event_id: &EventId) {
		self.rejectedeventids.remove(event_id);
	}

	pub(super) async fn is_event_rejected(&self, event_id: &EventId) -> bool {
		self.rejectedeventids.get(event_id).await.is_ok()
	}

	/// Removes any soft-fail or rejection markers applied to the target PDU
	pub(super) fn clear_pdu_markers(&self, event_id: &EventId) {
		self.unmark_event_rejected(event_id);
		self.unmark_event_soft_failed(event_id);
	}
}
