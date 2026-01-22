use itertools::Itertools;

use super::{algorithm::*, *};
use crate::memory_backend::MemoryStitcherBackend;

mod parser;

fn run_testcase(testcase: parser::TestCase<'_>) {
	let mut backend = MemoryStitcherBackend::default();

	for (index, phase) in testcase.into_iter().enumerate() {
		let stitcher = Stitcher::new(&backend);
		let batch = Batch::from_edges(&phase.batch);
		let updates = stitcher.stitch(&batch);

		println!();
		println!("===== phase {index}");
		for update in &updates.gap_updates {
			println!("update to gap {}:", update.key);
			println!("    new gap contents: {:?}", update.gap);
			println!("    inserted items: {:?}", update.inserted_items);
		}

		println!("expected new items: {:?}", &phase.order.new_items);
		println!("  actual new items: {:?}", &updates.new_items);
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

		if let Some(updated_gaps) = phase.updated_gaps {
			println!("expected events added to gaps: {updated_gaps:?}");
			println!("  actual events added to gaps: {:?}", updates.events_added_to_gaps);
			assert_eq!(
				updated_gaps, updates.events_added_to_gaps,
				"incorrect events added to gaps"
			);
		}

		backend.extend(updates);
		println!("extended ordering: {:?}", backend);

		for (expected, ref actual) in phase.order.iter().zip_eq(backend.iter()) {
			assert_eq!(
				expected, actual,
				"bad item in order, expected {expected:?} but got {actual:?}",
			);
		}
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
