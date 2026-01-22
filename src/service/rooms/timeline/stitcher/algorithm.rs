use std::{
	cmp::Ordering,
	collections::{BTreeSet, HashMap, HashSet},
};

use indexmap::IndexSet;
use itertools::Itertools;

use super::{Batch, Gap, OrderKey, StitchedItem, StitcherBackend};

/// Updates to a gap in the stitched order.
#[derive(Debug)]
pub(super) struct GapUpdate<'id, K: OrderKey> {
	/// The opaque key of the gap to update.
	pub key: K,
	/// The new contents of the gap. If this is empty, the gap should be
	/// deleted.
	pub gap: Gap,
	/// New items to insert after the gap. These items _should not_ be
	/// synchronized to clients.
	pub inserted_items: Vec<StitchedItem<'id>>,
}

/// Updates to the stitched order.
#[derive(Debug)]
pub(super) struct OrderUpdates<'id, K: OrderKey> {
	/// Updates to individual gaps. The items inserted by these updates _should
	/// not_ be synchronized to clients.
	pub gap_updates: Vec<GapUpdate<'id, K>>,
	/// New items to append to the end of the order. These items _should_ be
	/// synchronized to clients.
	pub new_items: Vec<StitchedItem<'id>>,
}

pub(super) struct Stitcher<'backend, B: StitcherBackend> {
	backend: &'backend B,
}

impl<B: StitcherBackend> Stitcher<'_, B> {
	pub(super) fn new(backend: &B) -> Stitcher<'_, B> { Stitcher { backend } }

	pub(super) fn stitch<'id>(&self, batch: Batch<'id>) -> OrderUpdates<'id, B::Key> {
		let mut gap_updates = Vec::new();
		let mut all_new_events: HashSet<&'id str> = HashSet::new();

		let mut remaining_events: IndexSet<_> = batch.events().collect();

		// 1: Find existing gaps which include IDs of events in `batch`
		let matching_gaps = self.backend.find_matching_gaps(batch.events());

		// Repeat steps 2-9 for each matching gap
		for (key, mut gap) in matching_gaps {
			// 2. Find events in `batch` which are mentioned in `gap`
			let matching_events = remaining_events.iter().filter(|id| gap.contains(**id));

			// 3. Create the to-insert list from the predecessor sets of each matching event
			let events_to_insert: Vec<_> = matching_events
				.filter_map(|event| batch.predecessors(event))
				.flat_map(|predecessors| predecessors.predecessor_set.iter())
				.filter(|event| remaining_events.contains(*event))
				.copied()
				.collect();

			all_new_events.extend(events_to_insert.iter());

			// 4. Remove the events in the to-insert list from `remaining_events` so they
			//    aren't processed again
			remaining_events.retain(|event| !events_to_insert.contains(event));

			// 5 and 6
			let inserted_items =
				self.sort_events_and_create_gaps(&batch, &all_new_events, events_to_insert);

			// 8. Update gap
			gap.retain(|id| !batch.contains(id));

			// 7 and 9. Append to-insert list and delete gap if empty
			// (the actual work of doing this is handled by the callee)
			gap_updates.push(GapUpdate { key: key.clone(), gap, inserted_items });
		}

		// 10. Append remaining events and gaps

		all_new_events.extend(remaining_events.iter());
		let new_items =
			self.sort_events_and_create_gaps(&batch, &all_new_events, remaining_events);

		OrderUpdates { gap_updates, new_items }
	}

	fn sort_events_and_create_gaps<'id>(
		&self,
		batch: &Batch<'id>,
		all_new_events: &HashSet<&'id str>,
		events_to_insert: impl IntoIterator<Item = &'id str>,
	) -> Vec<StitchedItem<'id>> {
		// 5. Sort the to-insert list with DAG;received order
		let events_to_insert = events_to_insert
			.into_iter()
			.sorted_by(batch.compare_by_dag_received())
			.collect_vec();

		let mut items = Vec::with_capacity(
			events_to_insert.capacity() + events_to_insert.capacity().div_euclid(2),
		);

		for event in events_to_insert {
			let missing_prev_events: HashSet<String> = batch
				.predecessors(event)
				.expect("events in to_insert should be in batch")
				.prev_events
				.iter()
				.filter(|prev_event| {
					!(batch.contains(prev_event)
						|| all_new_events.contains(*prev_event)
						|| self.backend.event_exists(prev_event))
				})
				.map(|id| String::from(*id))
				.collect();

			if !missing_prev_events.is_empty() {
				items.push(StitchedItem::Gap(missing_prev_events));
			}

			items.push(StitchedItem::Event(event));
		}

		items
	}
}
