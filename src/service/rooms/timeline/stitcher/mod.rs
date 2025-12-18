use std::collections::{BTreeMap, HashSet};

use ruma::{EventId, OwnedEventId};

pub(super) mod algorithm;

/// A gap in the stitched order.
pub(super) type Gap = HashSet<OwnedEventId>;

pub(super) enum StitchedItem<'id> {
	Event(&'id EventId),
	Gap(Gap),
}

/// An opaque key returned by a [`StitcherBackend`] to identify an item in its
/// order.
pub(super) trait OrderKey: Eq + Clone {}

pub(super) trait StitcherBackend {
	type Key: OrderKey;

	fn find_matching_gaps<'a>(
		&'a self,
		events: impl Iterator<Item = &'a EventId>,
	) -> impl Iterator<Item = (Self::Key, Gap)>;

	fn event_exists<'a>(&'a self, event: &'a EventId) -> bool;
}

/// An ordered map from an event ID to its `prev_events`.
pub(super) type EventEdges<'id> = BTreeMap<&'id EventId, HashSet<&'id EventId>>;

/// Information about the `prev_events` of an event.
/// This struct does not store the ID of the event itself.
struct EventPredecessors<'id> {
	/// The `prev_events` of the event.
	pub prev_events: HashSet<&'id EventId>,
	/// The predecessor set of the event. This is a superset of
	/// [`EventPredecessors::prev_events`]. See [`Batch::find_predecessor_set`]
	/// for details.
	pub predecessor_set: HashSet<&'id EventId>,
}

pub(super) struct Batch<'id> {
	events: BTreeMap<&'id EventId, EventPredecessors<'id>>,
}

impl<'id> Batch<'id> {
	pub(super) fn from_edges<'a>(edges: EventEdges<'a>) -> Batch<'a> {
		let mut events = BTreeMap::new();

		for (event, prev_events) in edges.iter() {
			let predecessor_set = Self::find_predecessor_set(event, &edges);

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
	fn find_predecessor_set<'a>(
		event: &'a EventId,
		edges: &EventEdges<'a>,
	) -> HashSet<&'a EventId> {
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

	fn events(&self) -> impl Iterator<Item = &'id EventId> { self.events.keys().copied() }

	fn contains(&self, event: &'id EventId) -> bool { self.events.contains_key(event) }

	fn predecessors(&self, event: &EventId) -> Option<&EventPredecessors<'id>> {
		self.events.get(event)
	}
}
