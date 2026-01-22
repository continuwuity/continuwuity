use std::collections::HashSet;

use indexmap::IndexMap;

use super::StitchedItem;

pub(super) type TestEventId<'id> = &'id str;

pub(super) type TestGap<'id> = HashSet<TestEventId<'id>>;

#[derive(Debug)]
pub(super) enum TestStitchedItem<'id> {
	Event(TestEventId<'id>),
	Gap(TestGap<'id>),
}

impl PartialEq<StitchedItem<'_>> for TestStitchedItem<'_> {
	fn eq(&self, other: &StitchedItem<'_>) -> bool {
		match (self, other) {
			| (TestStitchedItem::Event(lhs), StitchedItem::Event(rhs)) => lhs == rhs,
			| (TestStitchedItem::Gap(lhs), StitchedItem::Gap(rhs)) =>
				lhs.iter().all(|id| rhs.contains(*id)),
			| _ => false,
		}
	}
}

pub(super) type TestCase<'id> = Vec<Phase<'id>>;

pub(super) struct Phase<'id> {
	pub batch: Batch<'id>,
	pub order: Order<'id>,
	pub updated_gaps: Option<HashSet<TestEventId<'id>>>,
}

pub(super) type Batch<'id> = IndexMap<TestEventId<'id>, HashSet<TestEventId<'id>>>;

pub(super) struct Order<'id> {
	pub inserted_items: Vec<TestStitchedItem<'id>>,
	pub new_items: Vec<TestStitchedItem<'id>>,
}

impl<'id> Order<'id> {
	pub(super) fn iter(&self) -> impl Iterator<Item = &TestStitchedItem<'id>> {
		self.inserted_items.iter().chain(self.new_items.iter())
	}
}

peg::parser! {
	grammar testcase() for str {
		/// Parse whitespace.
		rule _ -> () = quiet! { $([' '])* {} }

		/// Parse empty lines and comments.
		rule newline() -> () = quiet! { (("#" [^'\n']*)? "\n")+ {} }

		/// Parse an "event ID" in a test case, which may only consist of ASCII letters and numbers.
		rule event_id() -> TestEventId<'input>
			= quiet! { id:$([char if char.is_ascii_alphanumeric()]+) { id } }
			  / expected!("event id")

		/// Parse a gap in the order section.
		rule gap() -> TestGap<'input>
			= "-" events:event_id() ++ "," { events.into_iter().collect() }

		/// Parse either an event id or a gap.
		rule stitched_item() -> TestStitchedItem<'input> =
			id:event_id() { TestStitchedItem::Event(id) }
			/ gap:gap() { TestStitchedItem::Gap(gap) }

		/// Parse an event line in the batch section, mapping an event name to zero or one prev events.
		/// The prev events are merged together by [`batch()`].
		rule batch_event() -> (TestEventId<'input>, Option<TestEventId<'input>>)
			= id:event_id() prev:(_ "-->" _ prev:event_id() { prev })? { (id, prev) }

		/// Parse the batch section of a phase.
		rule batch() -> Batch<'input>
			= events:batch_event() ++ newline() {
				/*
				Repeated event lines need to be merged together. For example,

				A --> B
				A --> C

				represents a _single_ event `A` with two prev events, `B` and `C`.
				*/
				events.into_iter()
					.fold(IndexMap::new(), |mut batch: Batch<'_>, (id, prev_event)| {
						// Find the prev events set of this event in the batch.
						// If it doesn't exist, make a new empty one.
						let mut prev_events = batch.entry(id).or_default();
						// If this event line defines a prev event to add, insert it into the set.
						if let Some(prev_event) = prev_event {
							prev_events.insert(prev_event);
						}

						batch
					})
			}

		rule order() -> Order<'input> =
			items:(item:stitched_item() new:"*"? { (item, new.is_some()) }) ** newline()
			{
				let (mut inserted_items, mut new_items) = (vec![], vec![]);

				for (item, new) in items {
					if new {
						new_items.push(item);
					} else {
						inserted_items.push(item);
					}
				}

				Order {
					inserted_items,
					new_items,
				}
			}

		rule updated_gaps() -> HashSet<TestEventId<'input>> =
			events:event_id() ++ newline() { events.into_iter().collect() }

		rule phase() -> Phase<'input> =
					  "=== when we receive these events ==="
			newline() batch:batch()
			newline() "=== then we arrange into this order ==="
			newline() order:order()
			updated_gaps:(
			newline() "=== and we notify about these gaps ==="
			newline() updated_gaps:updated_gaps() { updated_gaps }
			)?
			{ Phase { batch, order, updated_gaps } }

		pub rule testcase() -> TestCase<'input> = phase() ++ newline()
	}
}

pub(super) fn parse<'input>(input: &'input str) -> TestCase<'input> {
	testcase::testcase(input.trim_ascii_end()).expect("parse error")
}
