use std::{cmp::Ordering, collections::HashSet};

use indexmap::IndexMap;

pub mod algorithm;
#[cfg(test)]
mod test;
pub use algorithm::*;

/// A gap in the stitched order.
pub type Gap = HashSet<String>;

/// An item in the stitched order.
#[derive(Debug)]
pub enum StitchedItem<'id> {
	/// A single event.
	Event(&'id str),
	/// A gap representing one or more missing events.
	Gap(Gap),
}

/// An opaque key returned by a [`StitcherBackend`] to identify an item in its
/// order.
pub trait OrderKey: Eq + Clone {}

impl<T: Eq + Clone> OrderKey for T {}

/// A trait providing read-only access to an existing stitched order.
pub trait StitcherBackend {
	type Key: OrderKey;

	/// Return all gaps containing one or more events listed in `events`.
	fn find_matching_gaps<'a>(
		&'a self,
		events: impl Iterator<Item = &'a str>,
	) -> impl Iterator<Item = (Self::Key, Gap)>;

	/// Return whether an event exists in the stitched order.
	fn event_exists<'a>(&'a self, event: &'a str) -> bool;
}

/// An ordered map from an event ID to its `prev_events`.
pub type EventEdges<'id> = IndexMap<&'id str, HashSet<&'id str>>;

/// Information about the `prev_events` of an event.
/// This struct does not store the ID of the event itself.
#[derive(Debug)]
struct EventPredecessors<'id> {
	/// The `prev_events` of the event.
	pub prev_events: HashSet<&'id str>,
	/// The predecessor set of the event. This is derived from, and a superset
	/// of, [`EventPredecessors::prev_events`]. See
	/// [`Batch::find_predecessor_set`] for details. It is cached in this
	/// struct for performance.
	pub predecessor_set: HashSet<&'id str>,
}

/// A batch of events to be inserted into the stitched order.
#[derive(Debug)]
pub struct Batch<'id> {
	events: IndexMap<&'id str, EventPredecessors<'id>>,
}

impl<'id> Batch<'id> {
	/// Create a new [`Batch`] from an [`EventEdges`].
	pub fn from_edges<'edges>(edges: &EventEdges<'edges>) -> Batch<'edges> {
		let mut events = IndexMap::new();

		for (event, prev_events) in edges {
			let predecessor_set = Self::find_predecessor_set(event, edges);

			events.insert(*event, EventPredecessors {
				prev_events: prev_events.clone(),
				predecessor_set,
			});
		}

		Batch { events }
	}

	/// Build the predecessor set of `event` using `edges`. The predecessor set
	/// is a subgraph of the room's DAG which may be thought of as a tree
	/// rooted at `event` containing _only_ events which are included in
	/// `edges`. It is represented as a set and not a proper tree structure for
	/// efficiency.
	fn find_predecessor_set<'a>(event: &'a str, edges: &EventEdges<'a>) -> HashSet<&'a str> {
		// The predecessor set which we are building.
		let mut predecessor_set = HashSet::new();

		// The queue of events to check for membership in `remaining_events`.
		let mut events_to_check = vec![event];
		// Events which we have already checked and do not need to revisit.
		let mut events_already_checked = HashSet::new();

		while let Some(event) = events_to_check.pop() {
			// Don't add this event to the queue again.
			events_already_checked.insert(event);

			// If this event is in `edges`, add it to the predecessor set.
			if let Some(children) = edges.get(event) {
				predecessor_set.insert(event);

				// Also add all its `prev_events` to the queue. It's fine if some of them don't
				// exist in `edges` because they'll just be discarded when they're popped
				// off the queue.
				events_to_check.extend(
					children
						.iter()
						.filter(|event| !events_already_checked.contains(*event)),
				);
			}
		}

		predecessor_set
	}

	/// Iterate over all the events contained in this batch.
	fn events(&self) -> impl Iterator<Item = &'id str> { self.events.keys().copied() }

	/// Check whether an event exists in this batch.
	fn contains(&self, event: &'id str) -> bool { self.events.contains_key(event) }

	/// Return the predecessors of an event, if it exists in this batch.
	fn predecessors(&self, event: &str) -> Option<&EventPredecessors<'id>> {
		self.events.get(event)
	}

	/// Compare two events by DAG;received order.
	///
	/// If either event is in the other's predecessor set it comes first,
	/// otherwise they are sorted by which comes first in the batch.
	fn compare_by_dag_received(&self) -> impl FnMut(&&'id str, &&'id str) -> Ordering {
		|a, b| {
			if self
				.predecessors(a)
				.is_some_and(|it| it.predecessor_set.contains(b))
			{
				Ordering::Greater
			} else if self
				.predecessors(b)
				.is_some_and(|it| it.predecessor_set.contains(a))
			{
				Ordering::Less
			} else {
				let a_index = self
					.events
					.get_index_of(a)
					.expect("a should be in this batch");
				let b_index = self
					.events
					.get_index_of(b)
					.expect("b should be in this batch");

				a_index.cmp(&b_index)
			}
		}
	}
}
