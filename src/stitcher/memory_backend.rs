use std::{
	fmt::Debug,
	sync::atomic::{AtomicU64, Ordering},
};

use crate::{Gap, OrderUpdates, StitchedItem, StitcherBackend};

/// A version of [`StitchedItem`] which owns event IDs.
#[derive(Debug)]
enum MemoryStitcherItem {
	Event(String),
	Gap(Gap),
}

impl From<StitchedItem<'_>> for MemoryStitcherItem {
	fn from(value: StitchedItem) -> Self {
		match value {
			| StitchedItem::Event(id) => MemoryStitcherItem::Event(id.to_string()),
			| StitchedItem::Gap(gap) => MemoryStitcherItem::Gap(gap),
		}
	}
}

impl<'id> From<&'id MemoryStitcherItem> for StitchedItem<'id> {
	fn from(value: &'id MemoryStitcherItem) -> Self {
		match value {
			| MemoryStitcherItem::Event(id) => StitchedItem::Event(id),
			| MemoryStitcherItem::Gap(gap) => StitchedItem::Gap(gap.clone()),
		}
	}
}

/// A stitcher backend which holds a stitched ordering in RAM.
#[derive(Default)]
pub struct MemoryStitcherBackend {
	items: Vec<(u64, MemoryStitcherItem)>,
	counter: AtomicU64,
}

impl MemoryStitcherBackend {
	fn next_id(&self) -> u64 { self.counter.fetch_add(1, Ordering::Relaxed) }

	/// Extend this ordering with new updates.
	pub fn extend(&mut self, results: OrderUpdates<'_, <Self as StitcherBackend>::Key>) {
		for update in results.gap_updates {
			let Some(gap_index) = self.items.iter().position(|(key, _)| *key == update.key)
			else {
				panic!("bad update key {}", update.key);
			};

			let insertion_index = if update.gap.is_empty() {
				self.items.remove(gap_index);
				gap_index
			} else {
				match self.items.get_mut(gap_index) {
					| Some((_, MemoryStitcherItem::Gap(gap))) => {
						*gap = update.gap;
					},
					| Some((key, other)) => {
						panic!("expected item with key {key} to be a gap, it was {other:?}");
					},
					| None => unreachable!("we just checked that this index is valid"),
				}
				gap_index.checked_add(1).expect(
					"should never allocate usize::MAX ids. what kind of test are you running",
				)
			};

			let to_insert: Vec<_> = update
				.inserted_items
				.into_iter()
				.map(|item| (self.next_id(), item.into()))
				.collect();
			self.items
				.splice(insertion_index..insertion_index, to_insert.into_iter())
				.for_each(drop);
		}

		let new_items: Vec<_> = results
			.new_items
			.into_iter()
			.map(|item| (self.next_id(), item.into()))
			.collect();
		self.items.extend(new_items);
	}

	/// Iterate over the items in this ordering.
	pub fn iter(&self) -> impl Iterator<Item = StitchedItem<'_>> {
		self.items.iter().map(|(_, item)| item.into())
	}

	/// Clear this ordering.
	pub fn clear(&mut self) { self.items.clear(); }
}

impl StitcherBackend for MemoryStitcherBackend {
	type Key = u64;

	fn find_matching_gaps<'a>(
		&'a self,
		events: impl Iterator<Item = &'a str>,
	) -> impl Iterator<Item = (Self::Key, Gap)> {
		// nobody cares about test suite performance right
		let mut gaps = vec![];

		for event in events {
			for (key, item) in &self.items {
				if let MemoryStitcherItem::Gap(gap) = item
					&& gap.contains(event)
				{
					gaps.push((*key, gap.clone()));
				}
			}
		}

		gaps.into_iter()
	}

	fn event_exists<'a>(&'a self, event: &'a str) -> bool {
		self.items
			.iter()
			.any(|item| matches!(&item.1, MemoryStitcherItem::Event(id) if event == id))
	}
}

impl Debug for MemoryStitcherBackend {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_list().entries(self.iter()).finish()
	}
}
