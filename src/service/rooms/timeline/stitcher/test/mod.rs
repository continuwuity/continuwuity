use std::sync::atomic::{AtomicU64, Ordering};

use itertools::Itertools;

use super::{algorithm::*, *};
use crate::rooms::timeline::stitcher::algorithm::Stitcher;

mod parser;

#[derive(Default)]
struct TestStitcherBackend<'id> {
	items: Vec<(u64, StitchedItem<'id>)>,
	counter: AtomicU64,
}

impl<'id> TestStitcherBackend<'id> {
	fn next_id(&self) -> u64 { self.counter.fetch_add(1, Ordering::Relaxed) }

	fn extend(&mut self, results: OrderUpdates<'id, <Self as StitcherBackend>::Key>) {
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
					| Some((_, StitchedItem::Gap(gap))) => {
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
				.map(|item| (self.next_id(), item))
				.collect();
			self.items
				.splice(insertion_index..insertion_index, to_insert.into_iter())
				.for_each(drop);
		}

		let new_items: Vec<_> = results
			.new_items
			.into_iter()
			.map(|item| (self.next_id(), item))
			.collect();
		self.items.extend(new_items);
	}

	fn iter(&self) -> impl Iterator<Item = &StitchedItem<'id>> {
		self.items.iter().map(|(_, item)| item)
	}
}

impl StitcherBackend for TestStitcherBackend<'_> {
	type Key = u64;

	fn find_matching_gaps<'a>(
		&'a self,
		events: impl Iterator<Item = &'a str>,
	) -> impl Iterator<Item = (Self::Key, Gap)> {
		// nobody cares about test suite performance right
		let mut gaps = vec![];

		for event in events {
			for (key, item) in &self.items {
				if let StitchedItem::Gap(gap) = item
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
			.any(|item| matches!(item.1, StitchedItem::Event(id) if event == id))
	}
}

fn run_testcase(testcase: parser::TestCase<'_>) {
	let mut backend = TestStitcherBackend::default();

	for (index, phase) in testcase.into_iter().enumerate() {
		let stitcher = Stitcher::new(&backend);
		let batch = Batch::from_edges(&phase.batch);
		let updates = stitcher.stitch(&batch);

		println!();
		println!("===== phase {index}");
		println!("expected new items: {:?}", &phase.order.new_items);
		println!("  actual new items: {:?}", &updates.new_items);
		for update in &updates.gap_updates {
			println!("update to gap {}:", update.key);
			println!("    new gap contents: {:?}", update.gap);
			println!("    new items: {:?}", update.inserted_items);
		}

		for (expected, actual) in phase
			.order
			.new_items
			.iter()
			.zip_eq(updates.new_items.iter())
		{
			assert_eq!(
				expected, actual,
				"bad new item, expected {expected:?} but got {actual:?}"
			);
		}

		println!("ordering: {:?}", backend.items);
		backend.extend(updates);

		for (expected, actual) in phase.order.iter().zip_eq(backend.iter()) {
			assert_eq!(
				expected, actual,
				"bad item in order, expected {expected:?} but got {actual:?}",
			);
		}

		// TODO gap notification
	}
}

macro_rules! testcase {
	($index:literal : $id:ident) => {
		#[test]
		fn $id() {
			let testcase = parser::parse(include_str!(concat!(
				"./testcases/",
				$index,
				"-",
				stringify!($id),
				".stitched"
			)));

			run_testcase(testcase);
		}
	};
}

testcase!("001": receiving_new_events);
testcase!("002": recovering_after_netsplit);
testcase!("zzz": being_before_a_gap_item_beats_being_after_an_existing_item_multiple);
testcase!("zzz": being_before_a_gap_item_beats_being_after_an_existing_item);
testcase!("zzz": chains_are_reordered_using_prev_events);
testcase!("zzz": empty_then_simple_chain);
testcase!("zzz": empty_then_two_chains_interleaved);
testcase!("zzz": empty_then_two_chains);
testcase!("zzz": filling_in_a_gap_with_a_batch_containing_gaps);
testcase!("zzz": gaps_appear_before_events_referring_to_them_received_order);
testcase!("zzz": gaps_appear_before_events_referring_to_them);
testcase!("zzz": if_prev_events_determine_order_they_override_received);
testcase!("zzz": insert_into_first_of_several_gaps);
testcase!("zzz": insert_into_last_of_several_gaps);
testcase!("zzz": insert_into_middle_of_several_gaps);
testcase!("zzz": linked_events_are_split_across_gaps);
testcase!("zzz": linked_events_in_a_diamond_are_split_across_gaps);
testcase!("zzz": middle_of_batch_matches_gap_and_end_of_batch_matches_end);
testcase!("zzz": middle_of_batch_matches_gap);
testcase!("zzz": multiple_events_referring_to_the_same_missing_event_first_has_more);
testcase!("zzz": multiple_events_referring_to_the_same_missing_event);
testcase!("zzz": multiple_events_referring_to_the_same_missing_event_with_more);
testcase!("zzz": multiple_missing_prev_events_turn_into_a_single_gap);
testcase!("zzz": partially_filling_a_gap_leaves_it_before_new_nodes);
testcase!("zzz": partially_filling_a_gap_with_two_events);
testcase!("zzz": received_order_wins_within_a_subgroup_if_no_prev_event_chain);
testcase!("zzz": subgroups_are_processed_in_first_received_order);
