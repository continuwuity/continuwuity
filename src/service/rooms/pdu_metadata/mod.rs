mod bundled_aggregations;
mod data;
use std::{collections::HashSet, sync::Arc};

use conduwuit::{
	Result,
	matrix::{PduCount, PduEvent},
	result::LogErr,
	utils::{IterStream, ReadyExt, stream::WidebandExt},
};
use futures::{StreamExt, future::try_join};
use ruma::{EventId, RoomId, UserId, api::Direction};

use self::data::Data;
use crate::{
	Dep,
	rooms::{self, short::ShortRoomId, timeline::PdusIterItem},
};

pub struct Service {
	services: Services,
	db: Data,
}

struct Services {
	short: Dep<rooms::short::Service>,
	timeline: Dep<rooms::timeline::Service>,
	state_accessor: Dep<rooms::state_accessor::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				short: args.depend::<rooms::short::Service>("rooms::short"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
				state_accessor: args
					.depend::<rooms::state_accessor::Service>("rooms::state_accessor"),
			},
			db: Data::new(&args),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	#[tracing::instrument(skip(self, from, to), level = "debug")]
	pub fn add_relation(&self, from: PduCount, to: PduCount) {
		match (from, to) {
			| (PduCount::Normal(f), PduCount::Normal(t)) => self.db.add_relation(f, t),
			| _ => {
				// TODO: Relations with backfilled pdus
			},
		}
	}

	#[allow(clippy::too_many_arguments)]
	pub async fn get_relations<'a, F>(
		&'a self,
		user_id: &'a UserId,
		room_id: &'a RoomId,
		target: &'a EventId,
		from: PduCount,
		limit: usize,
		max_depth: u8,
		dir: Direction,
		matches: F,
	) -> Vec<PdusIterItem>
	where
		F: Fn(&PduEvent) -> bool + Send + Sync,
	{
		let Some((room_id, target)) = self.relation_target(room_id, target).await else {
			return Vec::new();
		};

		// Direct relations are already in order
		if max_depth == 0 {
			return self
				.db
				.get_relations(room_id, target, from, dir)
				.ready_filter(|(_, pdu)| matches(pdu))
				.take(limit)
				.wide_then(|relation| self.finish_relation(user_id, relation))
				.collect()
				.await;
		}

		let scan_from = match dir {
			| Direction::Forward => PduCount::min(),
			| Direction::Backward => from,
		};

		// An event can relate to two others in the tree, like a thread reply
		// that also replies to another reply in the thread, so it's only kept
		// and looked through the first time it's found
		let mut seen = HashSet::new();
		let mut relations = Vec::new();
		let mut level: Vec<_> = self
			.db
			.get_relations(room_id, target, scan_from, dir)
			.collect()
			.await;
		seen.extend(level.iter().map(|(count, _)| *count));
		for _ in 0..max_depth {
			let next: Vec<Vec<_>> = level
				.iter()
				.map(|(count, _)| *count)
				.stream()
				.wide_then(|target| {
					self.db
						.get_relations(room_id, target, scan_from, dir)
						.collect::<Vec<_>>()
				})
				.collect()
				.await;
			relations.append(&mut level);
			level = next
				.into_iter()
				.flatten()
				.filter(|(count, _)| seen.insert(*count))
				.collect();
		}
		relations.append(&mut level);

		relations
			.retain(|(count, pdu)| PduCount::Normal(*count).is_after(from, dir) && matches(pdu));
		relations.sort_unstable_by(|(a, _), (b, _)| match dir {
			| Direction::Forward => a.cmp(b),
			| Direction::Backward => b.cmp(a),
		});
		relations.truncate(limit);

		relations
			.into_iter()
			.stream()
			.wide_then(|relation| self.finish_relation(user_id, relation))
			.collect()
			.await
	}

	/// The short room ID and count of `target`
	async fn relation_target(
		&self,
		room_id: &RoomId,
		target: &EventId,
	) -> Option<(ShortRoomId, u64)> {
		let room_id = self.services.short.get_shortroomid(room_id);
		let target = self.services.timeline.get_pdu_count(target);

		match try_join(room_id, target).await {
			| Ok((room_id, PduCount::Normal(target))) => Some((room_id, target)),
			// TODO: Support backfilled relations
			| _ => None,
		}
	}

	async fn finish_relation(
		&self,
		user_id: &UserId,
		(count, pdu): (u64, PduEvent),
	) -> PdusIterItem {
		(PduCount::Normal(count), self.with_unsigned(user_id, pdu).await)
	}

	async fn with_unsigned(&self, user_id: &UserId, mut pdu: PduEvent) -> PduEvent {
		let ctx = self
			.services
			.timeline
			.get_unsigned_context(&pdu, Some(user_id))
			.await;
		pdu.set_unsigned(Some(ctx)).log_err().ok();

		pdu
	}

	#[inline]
	#[tracing::instrument(skip(self), level = "debug")]
	pub fn mark_event_soft_failed(&self, event_id: &EventId) {
		self.db.mark_event_soft_failed(event_id);
	}

	#[inline]
	#[tracing::instrument(skip(self), level = "debug")]
	pub async fn is_event_soft_failed(&self, event_id: &EventId) -> bool {
		self.db.is_event_soft_failed(event_id).await
	}

	pub async fn is_event_rejected(&self, event_id: &EventId) -> bool {
		self.db.is_event_rejected(event_id).await
	}

	pub fn mark_event_rejected(&self, event_id: &EventId) {
		self.db.mark_event_rejected(event_id);
	}

	pub fn unmark_event_soft_failed(&self, event_id: &EventId) {
		self.db.unmark_event_soft_failed(event_id);
	}

	pub fn unmark_event_rejected(&self, event_id: &EventId) {
		self.db.unmark_event_rejected(event_id);
	}

	/// Returns true if the event is neither soft-failed nor rejected.
	pub async fn is_event_accepted(&self, event_id: &EventId) -> bool {
		!self.db.is_event_rejected(event_id).await
			&& !self.db.is_event_soft_failed(event_id).await
	}

	pub fn clear_pdu_markers(&self, event_id: &EventId) { self.db.clear_pdu_markers(event_id); }
}
