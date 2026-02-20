#![cfg_attr(test, allow(warnings))]

pub(crate) mod error;
pub mod event_auth;
mod power_levels;
mod room_version;

#[cfg(test)]
mod test_utils;

#[cfg(test)]
mod benches;

use std::{
	borrow::Borrow,
	cmp::{Ordering, Reverse},
	collections::{BinaryHeap, HashMap, HashSet},
	hash::{BuildHasher, Hash},
};

use futures::{Future, FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt, future};
use ruma::{
	EventId, Int, MilliSecondsSinceUnixEpoch, OwnedEventId, RoomVersionId,
	events::{
		StateEventType, TimelineEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
	int,
};
use serde_json::from_str as from_json_str;

pub(crate) use self::error::Error;
use self::power_levels::PowerLevelsContentFields;
pub use self::{
	event_auth::{auth_check, auth_types_for_event},
	room_version::RoomVersion,
};
use crate::{
	debug, debug_error, err,
	matrix::{Event, StateKey},
	state_res::room_version::StateResolutionVersion,
	trace,
	utils::stream::{BroadbandExt, IterStream, ReadyExt, TryBroadbandExt, WidebandExt},
	warn,
};

/// A mapping of event type and state_key to some value `T`, usually an
/// `EventId`.
pub type StateMap<T> = HashMap<TypeStateKey, T>;
pub type StateMapItem<T> = (TypeStateKey, T);
pub type TypeStateKey = (StateEventType, StateKey);

type Result<T, E = Error> = crate::Result<T, E>;

/// Resolve sets of state events as they come in.
///
/// Internally `StateResolution` builds a graph and an auth chain to allow for
/// state conflict resolution.
///
/// ## Arguments
///
/// * `state_sets` - The incoming state to resolve. Each `StateMap` represents a
///   possible fork in the state of a room.
///
/// * `auth_chain_sets` - The full recursive set of `auth_events` for each event
///   in the `state_sets`.
///
/// * `event_fetch` - Any event not found in the `event_map` will defer to this
///   closure to find the event.
///
/// ## Invariants
///
/// The caller of `resolve` must ensure that all the events are from the same
/// room. Although this function takes a `RoomId` it does not check that each
/// event is part of the same room.
//#[tracing::instrument(level = "debug", skip(state_sets, auth_chain_sets,
//#[tracing::instrument(level event_fetch))]
pub async fn resolve<'a, Pdu, Sets, SetIter, Hasher, Fetch, FetchFut, Exists, ExistsFut>(
	room_version: &RoomVersionId,
	state_sets: Sets,
	auth_chain_sets: &'a [HashSet<OwnedEventId, Hasher>],
	event_fetch: &Fetch,
	event_exists: &Exists,
) -> Result<StateMap<OwnedEventId>>
where
	Fetch: Fn(OwnedEventId) -> FetchFut + Sync,
	FetchFut: Future<Output = Option<Pdu>> + Send,
	Exists: Fn(OwnedEventId) -> ExistsFut + Sync,
	ExistsFut: Future<Output = bool> + Send,
	Sets: IntoIterator<IntoIter = SetIter> + Send,
	SetIter: Iterator<Item = &'a StateMap<OwnedEventId>> + Clone + Send,
	Hasher: BuildHasher + Send + Sync,
	Pdu: Event + Clone + Send + Sync,
	for<'b> &'b Pdu: Event + Send,
{
	use RoomVersionId::*;
	let stateres_version = match room_version {
		| V2 | V3 | V4 | V5 | V6 | V7 | V8 | V9 | V10 | V11 => StateResolutionVersion::V2,
		| _ => StateResolutionVersion::V2_1,
	};
	debug!(version = ?stateres_version, "State resolution starting");

	// Split non-conflicting and conflicting state
	let (unconflicted, conflicting) = separate(state_sets.into_iter());

	debug!(count = unconflicted.len(), "non-conflicting events");
	trace!(map = ?unconflicted, "non-conflicting events");

	if conflicting.is_empty() {
		debug!("no conflicting state found");
		return Ok(unconflicted);
	}

	debug!(count = conflicting.len(), "conflicting events");
	trace!(map = ?conflicting, "conflicting events");
	let (conflicted_state_subgraph, initial_state) =
		if stateres_version == StateResolutionVersion::V2_1 {
			let csg = calculate_conflicted_subgraph(&conflicting, event_fetch)
				.await
				.ok_or_else(|| {
					Error::InvalidPdu("Failed to calculate conflicted subgraph".to_owned())
				})?;
			debug!(count = csg.len(), "conflicted subgraph");
			trace!(set = ?csg, "conflicted subgraph");
			(csg, HashMap::new())
		} else {
			(HashSet::new(), unconflicted.clone())
		};

	// `all_conflicted` contains unique items
	// synapse says `full_set = {eid for eid in full_conflicted_set if eid in
	// event_map}`
	// Hydra: Also consider the conflicted state subgraph
	let all_conflicted: HashSet<_> = get_auth_chain_diff(auth_chain_sets)
		.chain(conflicting.into_values().flatten().stream())
		.broad_filter_map(async |id| event_exists(id.clone()).await.then_some(id))
		.chain(conflicted_state_subgraph.into_iter().stream())
		.collect()
		.await;

	debug!(count = all_conflicted.len(), "full conflicted set");
	trace!(set = ?all_conflicted, "full conflicted set");

	// We used to check that all events are events from the correct room
	// this is now a check the caller of `resolve` must make.

	// Get only the control events with a state_key: "" or ban/kick event (sender !=
	// state_key)
	let control_events: Vec<_> = all_conflicted
		.iter()
		.stream()
		.wide_filter_map(async |id| {
			is_power_event_id(id, &event_fetch)
				.await
				.then_some(id.clone())
		})
		.collect()
		.await;

	// Sort the control events based on power_level/clock/event_id and
	// outgoing/incoming edges
	let sorted_control_levels =
		reverse_topological_power_sort(control_events, &all_conflicted, &event_fetch).await?;

	debug!(count = sorted_control_levels.len(), "power events");
	trace!(list = ?sorted_control_levels, "sorted power events");

	let room_version = RoomVersion::new(room_version)?;
	// Sequentially auth check each control event.
	let resolved_control = iterative_auth_check(
		&room_version,
		sorted_control_levels.iter().stream().map(AsRef::as_ref),
		initial_state,
		&event_fetch,
	)
	.await?;

	debug!(count = resolved_control.len(), "resolved power events");
	trace!(map = ?resolved_control, "resolved power events");

	// At this point the control_events have been resolved we now have to
	// sort the remaining events using the mainline of the resolved power level.
	let deduped_power_ev: HashSet<_> = sorted_control_levels.into_iter().collect();

	debug!(count = deduped_power_ev.len(), "deduped power events");
	trace!(set = ?deduped_power_ev, "deduped power events");

	// This removes the control events that passed auth and more importantly those
	// that failed auth
	let events_to_resolve: Vec<_> = all_conflicted
		.iter()
		.filter(|&id| !deduped_power_ev.contains(id))
		.cloned()
		.collect();

	debug!(count = events_to_resolve.len(), "events left to resolve");
	trace!(list = ?events_to_resolve, "events left to resolve");

	// This "epochs" power level event
	let power_levels_ty_sk = (StateEventType::RoomPowerLevels, StateKey::new());
	let power_event = resolved_control.get(&power_levels_ty_sk);

	trace!(event_id = ?power_event, "power event");

	let sorted_left_events =
		mainline_sort(&events_to_resolve, power_event.cloned(), &event_fetch).await?;

	trace!(list = ?sorted_left_events, "events left, sorted, running iterative auth check");

	let mut resolved_state = iterative_auth_check(
		&room_version,
		sorted_left_events.iter().stream().map(AsRef::as_ref),
		resolved_control, // The control events are added to the final resolved state
		&event_fetch,
	)
	.await?;

	// Ensure unconflicting state is in the final state
	resolved_state.extend(unconflicted);

	debug!("state resolution finished");
	trace!( map = ?resolved_state, "final resolved state" );

	Ok(resolved_state)
}

/// Split the events that have no conflicts from those that are conflicting.
///
/// The return tuple looks like `(unconflicted, conflicted)`.
///
/// State is determined to be conflicting if for the given key (StateEventType,
/// StateKey) there is not exactly one event ID. This includes missing events,
/// if one state_set includes an event that none of the other have this is a
/// conflicting event.
fn separate<'a, Id>(
	state_sets_iter: impl Iterator<Item = &'a StateMap<Id>>,
) -> (StateMap<Id>, StateMap<Vec<Id>>)
where
	Id: Clone + Eq + Hash + 'a,
{
	let mut state_set_count: usize = 0;
	let mut occurrences = HashMap::<_, HashMap<_, _>>::new();

	let state_sets_iter =
		state_sets_iter.inspect(|_| state_set_count = state_set_count.saturating_add(1));

	for (k, v) in state_sets_iter.flatten() {
		occurrences
			.entry(k)
			.or_default()
			.entry(v)
			.and_modify(|x: &mut usize| *x = x.saturating_add(1))
			.or_insert(1);
	}

	let mut unconflicted_state = StateMap::new();
	let mut conflicted_state = StateMap::new();

	for (k, v) in occurrences {
		for (id, occurrence_count) in v {
			if occurrence_count == state_set_count {
				unconflicted_state.insert((k.0.clone(), k.1.clone()), id.clone());
			} else {
				conflicted_state
					.entry((k.0.clone(), k.1.clone()))
					.and_modify(|x: &mut Vec<_>| x.push(id.clone()))
					.or_insert_with(|| vec![id.clone()]);
			}
		}
	}

	(unconflicted_state, conflicted_state)
}

/// Calculate the conflicted subgraph
async fn calculate_conflicted_subgraph<F, Fut, E>(
	conflicted: &StateMap<Vec<OwnedEventId>>,
	fetch_event: &F,
) -> Option<HashSet<OwnedEventId>>
where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	E: Event + Send + Sync,
{
	let conflicted_events: HashSet<_> = conflicted.values().flatten().cloned().collect();
	let mut subgraph: HashSet<OwnedEventId> = HashSet::new();
	let mut stack: Vec<Vec<OwnedEventId>> =
		vec![conflicted_events.iter().cloned().collect::<Vec<_>>()];
	let mut path: Vec<OwnedEventId> = Vec::new();
	let mut seen: HashSet<OwnedEventId> = HashSet::new();
	let next_event = |stack: &mut Vec<Vec<_>>, path: &mut Vec<_>| {
		while stack.last().is_some_and(Vec::is_empty) {
			stack.pop();
			path.pop();
		}
		stack.last_mut().and_then(Vec::pop)
	};
	while let Some(event_id) = next_event(&mut stack, &mut path) {
		path.push(event_id.clone());
		if subgraph.contains(&event_id) {
			if path.len() > 1 {
				subgraph.extend(path.iter().cloned());
			}
			path.pop();
			continue;
		}
		if conflicted_events.contains(&event_id) && path.len() > 1 {
			subgraph.extend(path.iter().cloned());
		}
		if seen.contains(&event_id) {
			path.pop();
			continue;
		}
		trace!(event_id = event_id.as_str(), "fetching event for its auth events");
		let evt = fetch_event(event_id.clone()).await;
		if evt.is_none() {
			err!("could not fetch event {} to calculate conflicted subgraph", event_id);
			path.pop();
			continue;
		}
		stack.push(
			evt.expect("checked")
				.auth_events()
				.map(ToOwned::to_owned)
				.collect(),
		);
		seen.insert(event_id);
	}
	Some(subgraph)
}

/// Returns a Vec of deduped EventIds that appear in some chains but not others.
#[allow(clippy::arithmetic_side_effects)]
fn get_auth_chain_diff<Id, Hasher>(
	auth_chain_sets: &[HashSet<Id, Hasher>],
) -> impl Stream<Item = Id> + Send + use<Id, Hasher>
where
	Id: Clone + Eq + Hash + Send,
	Hasher: BuildHasher + Send + Sync,
{
	let num_sets = auth_chain_sets.len();
	let mut id_counts: HashMap<Id, usize> = HashMap::new();
	for id in auth_chain_sets.iter().flatten() {
		*id_counts.entry(id.clone()).or_default() += 1;
	}

	id_counts
		.into_iter()
		.filter_map(move |(id, count)| (count < num_sets).then_some(id))
		.stream()
}

/// Events are sorted from "earliest" to "latest".
///
/// They are compared using the negative power level (reverse topological
/// ordering), the origin server timestamp and in case of a tie the `EventId`s
/// are compared lexicographically.
///
/// The power level is negative because a higher power level is equated to an
/// earlier (further back in time) origin server timestamp.
#[tracing::instrument(level = "debug", skip_all)]
async fn reverse_topological_power_sort<E, F, Fut>(
	events_to_sort: Vec<OwnedEventId>,
	auth_diff: &HashSet<OwnedEventId>,
	fetch_event: &F,
) -> Result<Vec<OwnedEventId>>
where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	E: Event + Send + Sync,
{
	debug!("reverse topological sort of power events");

	let mut graph = HashMap::new();
	for event_id in events_to_sort {
		add_event_and_auth_chain_to_graph(&mut graph, event_id, auth_diff, fetch_event).await;
	}

	// This is used in the `key_fn` passed to the lexico_topo_sort fn
	let event_to_pl: HashMap<_, _> = graph
		.keys()
		.cloned()
		.stream()
		.broad_filter_map(async |event_id| {
			let pl = get_power_level_for_sender(&event_id, fetch_event)
				.await
				.ok()?;

			Some((event_id, pl))
		})
		.inspect(|(event_id, pl)| {
			debug!(
				event_id = event_id.as_str(),
				power_level = i64::from(*pl),
				"found the power level of an event's sender",
			);
		})
		.collect()
		.boxed()
		.await;

	let fetcher = async |event_id: OwnedEventId| {
		let pl = *event_to_pl
			.get(&event_id)
			.ok_or_else(|| Error::NotFound(String::new()))?;

		let ev = fetch_event(event_id)
			.await
			.ok_or_else(|| Error::NotFound(String::new()))?;

		Ok((pl, ev.origin_server_ts()))
	};

	lexicographical_topological_sort(&graph, &fetcher).await
}

/// Sorts the event graph based on number of outgoing/incoming edges.
///
/// `key_fn` is used as to obtain the power level and age of an event for
/// breaking ties (together with the event ID).
#[tracing::instrument(level = "debug", skip_all)]
pub async fn lexicographical_topological_sort<Id, F, Fut, Hasher, S>(
	graph: &HashMap<Id, HashSet<Id, Hasher>, S>,
	key_fn: &F,
) -> Result<Vec<Id>>
where
	F: Fn(Id) -> Fut + Sync,
	Fut: Future<Output = Result<(Int, MilliSecondsSinceUnixEpoch)>> + Send,
	Id: Borrow<EventId> + Clone + Eq + Hash + Ord + Send + Sync,
	Hasher: BuildHasher + Default + Clone + Send + Sync,
	S: BuildHasher + Clone + Send + Sync,
{
	#[derive(PartialEq, Eq)]
	struct TieBreaker<'a, Id> {
		power_level: Int,
		origin_server_ts: MilliSecondsSinceUnixEpoch,
		event_id: &'a Id,
	}

	impl<Id> Ord for TieBreaker<'_, Id>
	where
		Id: Ord,
	{
		fn cmp(&self, other: &Self) -> Ordering {
			// NOTE: the power level comparison is "backwards" intentionally.
			// See the "Mainline ordering" section of the Matrix specification
			// around where it says the following:
			//
			// > for events `x` and `y`, `x < y` if [...]
			//
			// <https://spec.matrix.org/v1.12/rooms/v11/#definitions>
			other
				.power_level
				.cmp(&self.power_level)
				.then(self.origin_server_ts.cmp(&other.origin_server_ts))
				.then(self.event_id.cmp(other.event_id))
		}
	}

	impl<Id> PartialOrd for TieBreaker<'_, Id>
	where
		Id: Ord,
	{
		fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
	}

	debug!("starting lexicographical topological sort");

	// NOTE: an event that has no incoming edges happened most recently,
	// and an event that has no outgoing edges happened least recently.

	// NOTE: this is basically Kahn's algorithm except we look at nodes with no
	// outgoing edges, c.f.
	// https://en.wikipedia.org/wiki/Topological_sorting#Kahn's_algorithm

	// outdegree_map is an event referring to the events before it, the
	// more outdegree's the more recent the event.
	let mut outdegree_map = graph.clone();

	// The number of events that depend on the given event (the EventId key)
	// How many events reference this event in the DAG as a parent
	let mut reverse_graph: HashMap<_, HashSet<_, Hasher>> = HashMap::new();

	// Vec of nodes that have zero out degree, least recent events.
	let mut zero_outdegree = Vec::new();

	for (node, edges) in graph {
		if edges.is_empty() {
			let (power_level, origin_server_ts) = key_fn(node.clone()).await?;
			// The `Reverse` is because rusts `BinaryHeap` sorts largest -> smallest we need
			// smallest -> largest
			zero_outdegree.push(Reverse(TieBreaker {
				power_level,
				origin_server_ts,
				event_id: node,
			}));
		}

		reverse_graph.entry(node).or_default();
		for edge in edges {
			reverse_graph.entry(edge).or_default().insert(node);
		}
	}

	let mut heap = BinaryHeap::from(zero_outdegree);

	// We remove the oldest node (most incoming edges) and check against all other
	let mut sorted = vec![];
	// Destructure the `Reverse` and take the smallest `node` each time
	while let Some(Reverse(item)) = heap.pop() {
		let node = item.event_id;

		for &parent in reverse_graph
			.get(node)
			.expect("EventId in heap is also in reverse_graph")
		{
			// The number of outgoing edges this node has
			let out = outdegree_map
				.get_mut(parent.borrow())
				.expect("outdegree_map knows of all referenced EventIds");

			// Only push on the heap once older events have been cleared
			out.remove(node.borrow());
			if out.is_empty() {
				let (power_level, origin_server_ts) = key_fn(parent.clone()).await?;
				heap.push(Reverse(TieBreaker {
					power_level,
					origin_server_ts,
					event_id: parent,
				}));
			}
		}

		// synapse yields we push then return the vec
		sorted.push(node.clone());
	}

	Ok(sorted)
}

/// Find the power level for the sender of `event_id` or return a default value
/// of zero.
///
/// Do NOT use this any where but topological sort, we find the power level for
/// the eventId at the eventId's generation (we walk backwards to `EventId`s
/// most recent previous power level event).
async fn get_power_level_for_sender<E, F, Fut>(
	event_id: &EventId,
	fetch_event: &F,
) -> serde_json::Result<Int>
where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	E: Event + Send,
{
	debug!("fetch event ({event_id}) senders power level");

	let event = fetch_event(event_id.to_owned()).await;

	let auth_events = event.as_ref().map(Event::auth_events);

	let pl = auth_events
		.into_iter()
		.flatten()
		.stream()
		.broadn_filter_map(5, |aid| fetch_event(aid.to_owned()))
		.ready_find(|aev| is_type_and_key(aev, &TimelineEventType::RoomPowerLevels, ""))
		.await;

	let content: PowerLevelsContentFields = match pl {
		| None => return Ok(int!(0)),
		| Some(ev) => from_json_str(ev.content().get())?,
	};

	if let Some(ev) = event {
		if let Some(&user_level) = content.get_user_power(ev.sender()) {
			debug!("found {} at power_level {user_level}", ev.sender());
			return Ok(user_level);
		}
	}

	Ok(content.users_default)
}

/// Check the that each event is authenticated based on the events before it.
///
/// ## Returns
///
/// The `unconflicted_state` combined with the newly auth'ed events. So any
/// event that fails the `event_auth::auth_check` will be excluded from the
/// returned state map.
///
/// For each `events_to_check` event we gather the events needed to auth it from
/// the the `fetch_event` closure and verify each event using the
/// `event_auth::auth_check` function.
#[tracing::instrument(level = "trace", skip_all)]
async fn iterative_auth_check<'a, E, F, Fut, S>(
	room_version: &RoomVersion,
	events_to_check: S,
	unconflicted_state: StateMap<OwnedEventId>,
	fetch_event: &F,
) -> Result<StateMap<OwnedEventId>>
where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	S: Stream<Item = &'a EventId> + Send + 'a,
	E: Event + Clone + Send + Sync,
	for<'b> &'b E: Event + Send,
{
	debug!("starting iterative auth check");

	let events_to_check: Vec<_> = events_to_check
		.map(Result::Ok)
		.broad_and_then(async |event_id| {
			fetch_event(event_id.to_owned())
				.await
				.ok_or_else(|| Error::NotFound(format!("Failed to find {event_id}")))
		})
		.try_collect()
		.boxed()
		.await?;
	trace!(list = ?events_to_check, "events to check");
	if events_to_check.is_empty() {
		debug!("no events to check, returning unconflicted state");
		return Ok(unconflicted_state);
	}

	let auth_event_ids: HashSet<OwnedEventId> = events_to_check
		.iter()
		.flat_map(|event: &E| event.auth_events().map(ToOwned::to_owned))
		.collect();

	trace!(set = ?auth_event_ids, "auth event IDs to fetch");

	let auth_events: HashMap<OwnedEventId, E> = auth_event_ids
		.into_iter()
		.stream()
		.broad_filter_map(fetch_event)
		.map(|auth_event| (auth_event.event_id().to_owned(), auth_event))
		.collect()
		.boxed()
		.await;

	trace!(map = ?auth_events.keys().collect::<Vec<_>>(), "fetched auth events");

	let auth_events = &auth_events;
	// NOTE: in state resolution v2.1, auth checks should start with an empty state
	// map. It is the caller's job to do this. Previously, this function would
	// force an empty state map in this case, and this resulted in power events
	// going missing from the resolved state as they'd be discarded here.
	let mut resolved_state = unconflicted_state;
	for event in events_to_check {
		trace!(event_id = event.event_id().as_str(), "checking event");
		let state_key = event
			.state_key()
			.ok_or_else(|| Error::InvalidPdu("State event had no state key".to_owned()))?;

		let auth_types = auth_types_for_event(
			event.event_type(),
			event.sender(),
			Some(state_key),
			event.content(),
			room_version,
		)?;
		trace!(list = ?auth_types, event_id = event.event_id().as_str(), "auth types for event");

		let mut auth_state = StateMap::new();
		if room_version.room_ids_as_hashes {
			trace!("room version uses hashed IDs, manually fetching create event");
			let create_event_id_raw = event.room_id_or_hash().as_str().replace('!', "$");
			let create_event_id = EventId::parse(&create_event_id_raw).map_err(|e| {
				Error::InvalidPdu(format!(
					"Failed to parse create event ID from room ID/hash: {e}"
				))
			})?;
			let create_event = fetch_event(create_event_id.into())
				.await
				.ok_or_else(|| Error::NotFound("Failed to find create event".into()))?;
			auth_state.insert(create_event.event_type().with_state_key(""), create_event);
		}
		for aid in event.auth_events() {
			if let Some(ev) = auth_events.get(aid) {
				//TODO: synapse checks "rejected_reason" which is most likely related to
				// soft-failing
				trace!(event_id = aid.as_str(), "found auth event");
				auth_state.insert(
					ev.event_type()
						.with_state_key(ev.state_key().ok_or_else(|| {
							Error::InvalidPdu("State event had no state key".to_owned())
						})?),
					ev.clone(),
				);
			} else {
				warn!(event_id = aid.as_str(), "missing auth event");
			}
		}

		auth_types
			.iter()
			.stream()
			.ready_filter_map(|key| Some((key, resolved_state.get(key)?)))
			.filter_map(|(key, ev_id)| async move {
				if let Some(event) = auth_events.get(ev_id) {
					Some((key, event.clone()))
				} else {
					Some((key, fetch_event(ev_id.clone()).await?))
				}
			})
			.ready_for_each(|(key, event)| {
				//TODO: synapse checks "rejected_reason" is None here
				auth_state.insert(key.to_owned(), event);
			})
			.await;
		trace!(map = ?auth_state.keys().collect::<Vec<_>>(), event_id = event.event_id().as_str(), "auth state for event");

		debug!(event_id = event.event_id().as_str(), "Running auth checks");

		// The key for this is (eventType + a state_key of the signed token not sender)
		// so search for it
		let current_third_party = auth_state.iter().find_map(|(_, pdu)| {
			(*pdu.event_type() == TimelineEventType::RoomThirdPartyInvite).then_some(pdu)
		});

		let fetch_state = |ty: &StateEventType, key: &str| {
			future::ready(
				auth_state
					.get(&ty.with_state_key(key))
					.map(ToOwned::to_owned),
			)
		};

		let auth_result = auth_check(
			room_version,
			&event,
			current_third_party,
			fetch_state,
			&fetch_state(&StateEventType::RoomCreate, "")
				.await
				.expect("create event must exist"),
		)
		.await;

		match auth_result {
			| Ok(true) => {
				// add event to resolved state map
				trace!(
					event_id = event.event_id().as_str(),
					"event passed the authentication check, adding to resolved state"
				);
				resolved_state.insert(
					event.event_type().with_state_key(state_key),
					event.event_id().to_owned(),
				);
				trace!(map = ?resolved_state, "new resolved state");
			},
			| Ok(false) => {
				// synapse passes here on AuthError. We do not add this event to resolved_state.
				warn!("event {} failed the authentication check", event.event_id());
			},
			| Err(e) => {
				debug_error!("event {} failed the authentication check: {e}", event.event_id());
				return Err(e);
			},
		}
	}
	trace!(map = ?resolved_state, "final resolved state from iterative auth check");
	debug!("iterative auth check finished");
	Ok(resolved_state)
}

/// Returns the sorted `to_sort` list of `EventId`s based on a mainline sort
/// using the depth of `resolved_power_level`, the server timestamp, and the
/// eventId.
///
/// The depth of the given event is calculated based on the depth of it's
/// closest "parent" power_level event. If there have been two power events the
/// after the most recent are depth 0, the events before (with the first power
/// level as a parent) will be marked as depth 1. depth 1 is "older" than depth
/// 0.
async fn mainline_sort<E, F, Fut>(
	to_sort: &[OwnedEventId],
	resolved_power_level: Option<OwnedEventId>,
	fetch_event: &F,
) -> Result<Vec<OwnedEventId>>
where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	E: Event + Clone + Send + Sync,
{
	debug!("mainline sort of events");

	// There are no EventId's to sort, bail.
	if to_sort.is_empty() {
		return Ok(vec![]);
	}

	let mut mainline = vec![];
	let mut pl = resolved_power_level;
	while let Some(p) = pl {
		mainline.push(p.clone());

		let event = fetch_event(p.clone())
			.await
			.ok_or_else(|| Error::NotFound(format!("Failed to find {p}")))?;

		pl = None;
		for aid in event.auth_events() {
			let ev = fetch_event(aid.to_owned())
				.await
				.ok_or_else(|| Error::NotFound(format!("Failed to find {aid}")))?;

			if is_type_and_key(&ev, &TimelineEventType::RoomPowerLevels, "") {
				pl = Some(aid.to_owned());
				break;
			}
		}
	}

	let mainline_map: HashMap<_, _> = mainline
		.iter()
		.rev()
		.enumerate()
		.map(|(idx, eid)| ((*eid).clone(), idx))
		.collect();

	let order_map: HashMap<_, _> = to_sort
		.iter()
		.stream()
		.broad_filter_map(async |ev_id| {
			fetch_event(ev_id.clone()).await.map(|event| (event, ev_id))
		})
		.broad_filter_map(|(event, ev_id)| {
			get_mainline_depth(Some(event.clone()), &mainline_map, fetch_event)
				.map_ok(move |depth| (ev_id, (depth, event.origin_server_ts(), ev_id)))
				.map(Result::ok)
		})
		.collect()
		.boxed()
		.await;

	// Sort the event_ids by their depth, timestamp and EventId
	// unwrap is OK order map and sort_event_ids are from to_sort (the same Vec)
	let mut sort_event_ids: Vec<_> = order_map.keys().map(|&k| k.clone()).collect();

	sort_event_ids.sort_by_key(|sort_id| &order_map[sort_id]);

	Ok(sort_event_ids)
}

/// Get the mainline depth from the `mainline_map` or finds a power_level event
/// that has an associated mainline depth.
async fn get_mainline_depth<E, F, Fut>(
	mut event: Option<E>,
	mainline_map: &HashMap<OwnedEventId, usize>,
	fetch_event: &F,
) -> Result<usize>
where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	E: Event + Send + Sync,
{
	while let Some(sort_ev) = event {
		debug!(event_id = sort_ev.event_id().as_str(), "mainline");

		let id = sort_ev.event_id();
		if let Some(depth) = mainline_map.get(id) {
			return Ok(*depth);
		}

		event = None;
		for aid in sort_ev.auth_events() {
			let aev = fetch_event(aid.to_owned())
				.await
				.ok_or_else(|| Error::NotFound(format!("Failed to find {aid}")))?;

			if is_type_and_key(&aev, &TimelineEventType::RoomPowerLevels, "") {
				event = Some(aev);
				break;
			}
		}
	}
	// Did not find a power level event so we default to zero
	Ok(0)
}

async fn add_event_and_auth_chain_to_graph<E, F, Fut>(
	graph: &mut HashMap<OwnedEventId, HashSet<OwnedEventId>>,
	event_id: OwnedEventId,
	auth_diff: &HashSet<OwnedEventId>,
	fetch_event: &F,
) where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	E: Event + Send + Sync,
{
	let mut state = vec![event_id];
	while let Some(eid) = state.pop() {
		graph.entry(eid.clone()).or_default();
		let event = fetch_event(eid.clone()).await;
		let auth_events = event.as_ref().map(Event::auth_events).into_iter().flatten();

		// Prefer the store to event as the store filters dedups the events
		for aid in auth_events {
			if auth_diff.contains(aid) {
				if !graph.contains_key(aid) {
					state.push(aid.to_owned());
				}

				graph
					.get_mut(&eid)
					.expect("We just inserted this at the start of the while loop")
					.insert(aid.to_owned());
			}
		}
	}
}

async fn is_power_event_id<E, F, Fut>(event_id: &EventId, fetch: &F) -> bool
where
	F: Fn(OwnedEventId) -> Fut + Sync,
	Fut: Future<Output = Option<E>> + Send,
	E: Event + Send,
{
	match fetch(event_id.to_owned()).await.as_ref() {
		| Some(state) => is_power_event(state),
		| _ => false,
	}
}

fn is_type_and_key(ev: &impl Event, ev_type: &TimelineEventType, state_key: &str) -> bool {
	ev.event_type() == ev_type && ev.state_key() == Some(state_key)
}

fn is_power_event(event: &impl Event) -> bool {
	match event.event_type() {
		| TimelineEventType::RoomPowerLevels
		| TimelineEventType::RoomJoinRules
		| TimelineEventType::RoomCreate => event.state_key() == Some(""),
		| TimelineEventType::RoomMember => {
			if let Ok(content) = from_json_str::<RoomMemberEventContent>(event.content().get()) {
				if [MembershipState::Leave, MembershipState::Ban].contains(&content.membership) {
					return Some(event.sender().as_str()) != event.state_key();
				}
			}

			false
		},
		| _ => false,
	}
}

/// Convenience trait for adding event type plus state key to state maps.
pub trait EventTypeExt {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey);
}

impl EventTypeExt for StateEventType {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey) {
		(self, state_key.into())
	}
}

impl EventTypeExt for TimelineEventType {
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey) {
		(self.into(), state_key.into())
	}
}

impl<T> EventTypeExt for &T
where
	T: EventTypeExt + Clone,
{
	fn with_state_key(self, state_key: impl Into<StateKey>) -> (StateEventType, StateKey) {
		self.to_owned().with_state_key(state_key)
	}
}

#[cfg(test)]
mod tests {
	use std::collections::{HashMap, HashSet};

	use maplit::{hashmap, hashset};
	use rand::seq::SliceRandom;
	use ruma::{
		MilliSecondsSinceUnixEpoch, OwnedEventId, RoomVersionId,
		events::{
			StateEventType, TimelineEventType,
			room::join_rules::{JoinRule, RoomJoinRulesEventContent},
		},
		int, uint,
	};
	use serde_json::{json, value::to_raw_value as to_raw_json_value};

	use super::{
		StateMap, is_power_event,
		room_version::RoomVersion,
		test_utils::{
			INITIAL_EVENTS, TestStore, alice, bob, charlie, do_check, ella, event_id,
			member_content_ban, member_content_join, room_id, to_init_pdu_event, to_pdu_event,
			zara,
		},
	};
	use crate::{
		debug,
		matrix::{Event, EventTypeExt, Pdu as PduEvent},
		state_res::room_version::StateResolutionVersion,
		utils::stream::IterStream,
	};

	async fn test_event_sort() {
		use futures::future::ready;

		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let events = INITIAL_EVENTS();

		let event_map = events
			.values()
			.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.clone()))
			.collect::<StateMap<_>>();

		let auth_chain: HashSet<OwnedEventId> = HashSet::new();

		let power_events = event_map
			.values()
			.filter(|&pdu| is_power_event(&*pdu))
			.map(|pdu| pdu.event_id.clone())
			.collect::<Vec<_>>();

		let fetcher = |id| ready(events.get(&id).cloned());
		let sorted_power_events =
			super::reverse_topological_power_sort(power_events, &auth_chain, &fetcher)
				.await
				.unwrap();

		let resolved_power = super::iterative_auth_check(
			&RoomVersion::V6,
			sorted_power_events.iter().map(AsRef::as_ref).stream(),
			HashMap::new(), // unconflicted events
			&fetcher,
		)
		.await
		.expect("iterative auth check failed on resolved events");

		// don't remove any events so we know it sorts them all correctly
		let mut events_to_sort = events.keys().cloned().collect::<Vec<_>>();

		events_to_sort.shuffle(&mut rand::rng());

		let power_level = resolved_power
			.get(&(StateEventType::RoomPowerLevels, "".into()))
			.cloned();

		let sorted_event_ids = super::mainline_sort(&events_to_sort, power_level, &fetcher)
			.await
			.unwrap();

		assert_eq!(
			vec![
				"$CREATE:foo",
				"$IMA:foo",
				"$IPOWER:foo",
				"$IJR:foo",
				"$IMB:foo",
				"$IMC:foo",
				"$START:foo",
				"$END:foo"
			],
			sorted_event_ids
				.iter()
				.map(|id| id.to_string())
				.collect::<Vec<_>>()
		);
	}

	// NOTE(2025-09-17): Disabled due to unknown "create event must exist" bug
	// #[tokio::test]
	async fn test_sort() {
		for _ in 0..20 {
			// since we shuffle the eventIds before we sort them introducing randomness
			// seems like we should test this a few times
			test_event_sort().await;
		}
	}

	// NOTE(2025-09-17): Disabled due to unknown "create event must exist" bug
	//#[tokio::test]
	async fn ban_vs_power_level() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let events = &[
			to_init_pdu_event(
				"PA",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
			to_init_pdu_event(
				"MA",
				alice(),
				TimelineEventType::RoomMember,
				Some(alice().to_string().as_str()),
				member_content_join(),
			),
			to_init_pdu_event(
				"MB",
				alice(),
				TimelineEventType::RoomMember,
				Some(bob().to_string().as_str()),
				member_content_ban(),
			),
			to_init_pdu_event(
				"PB",
				bob(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
		];

		let edges = vec![vec!["END", "MB", "MA", "PA", "START"], vec!["END", "PA", "PB"]]
			.into_iter()
			.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
			.collect::<Vec<_>>();

		let expected_state_ids = vec!["PA", "MA", "MB"]
			.into_iter()
			.map(event_id)
			.collect::<Vec<_>>();

		do_check(events, edges, expected_state_ids).await;
	}

	#[tokio::test]
	async fn topic_basic() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let events = &[
			to_init_pdu_event(
				"T1",
				alice(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"PA1",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
			to_init_pdu_event(
				"T2",
				alice(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"PA2",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 0 } })).unwrap(),
			),
			to_init_pdu_event(
				"PB",
				bob(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
			to_init_pdu_event(
				"T3",
				bob(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
		];

		let edges =
			vec![vec!["END", "PA2", "T2", "PA1", "T1", "START"], vec!["END", "T3", "PB", "PA1"]]
				.into_iter()
				.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
				.collect::<Vec<_>>();

		let expected_state_ids = vec!["PA2", "T2"]
			.into_iter()
			.map(event_id)
			.collect::<Vec<_>>();

		do_check(events, edges, expected_state_ids).await;
	}

	#[tokio::test]
	async fn topic_reset() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let events = &[
			to_init_pdu_event(
				"T1",
				alice(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"PA",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
			to_init_pdu_event(
				"T2",
				bob(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"MB",
				alice(),
				TimelineEventType::RoomMember,
				Some(bob().to_string().as_str()),
				member_content_ban(),
			),
		];

		let edges = vec![vec!["END", "MB", "T2", "PA", "T1", "START"], vec!["END", "T1"]]
			.into_iter()
			.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
			.collect::<Vec<_>>();

		let expected_state_ids = vec!["T1", "MB", "PA"]
			.into_iter()
			.map(event_id)
			.collect::<Vec<_>>();

		do_check(events, edges, expected_state_ids).await;
	}

	#[tokio::test]
	async fn join_rule_evasion() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let events = &[
			to_init_pdu_event(
				"JR",
				alice(),
				TimelineEventType::RoomJoinRules,
				Some(""),
				to_raw_json_value(&RoomJoinRulesEventContent::new(JoinRule::Private)).unwrap(),
			),
			to_init_pdu_event(
				"ME",
				ella(),
				TimelineEventType::RoomMember,
				Some(ella().to_string().as_str()),
				member_content_join(),
			),
		];

		let edges = vec![vec!["END", "JR", "START"], vec!["END", "ME", "START"]]
			.into_iter()
			.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
			.collect::<Vec<_>>();

		let expected_state_ids = vec![event_id("JR")];

		do_check(events, edges, expected_state_ids).await;
	}

	#[tokio::test]
	async fn offtopic_power_level() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let events = &[
			to_init_pdu_event(
				"PA",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
			to_init_pdu_event(
				"PB",
				bob(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(
					&json!({ "users": { alice(): 100, bob(): 50, charlie(): 50 } }),
				)
				.unwrap(),
			),
			to_init_pdu_event(
				"PC",
				charlie(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50, charlie(): 0 } }))
					.unwrap(),
			),
		];

		let edges = vec![vec!["END", "PC", "PB", "PA", "START"], vec!["END", "PA"]]
			.into_iter()
			.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
			.collect::<Vec<_>>();

		let expected_state_ids = vec!["PC"].into_iter().map(event_id).collect::<Vec<_>>();

		do_check(events, edges, expected_state_ids).await;
	}

	#[tokio::test]
	async fn topic_setting() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let events = &[
			to_init_pdu_event(
				"T1",
				alice(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"PA1",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
			to_init_pdu_event(
				"T2",
				alice(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"PA2",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 0 } })).unwrap(),
			),
			to_init_pdu_event(
				"PB",
				bob(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
			),
			to_init_pdu_event(
				"T3",
				bob(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"MZ1",
				zara(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
			to_init_pdu_event(
				"T4",
				alice(),
				TimelineEventType::RoomTopic,
				Some(""),
				to_raw_json_value(&json!({})).unwrap(),
			),
		];

		let edges = vec![vec!["END", "T4", "MZ1", "PA2", "T2", "PA1", "T1", "START"], vec![
			"END", "MZ1", "T3", "PB", "PA1",
		]]
		.into_iter()
		.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
		.collect::<Vec<_>>();

		let expected_state_ids = vec!["T4", "PA2"]
			.into_iter()
			.map(event_id)
			.collect::<Vec<_>>();

		do_check(events, edges, expected_state_ids).await;
	}

	#[tokio::test]
	async fn test_event_map_none() {
		use futures::future::ready;

		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let mut store = TestStore::<PduEvent>(hashmap! {});

		// build up the DAG
		let (state_at_bob, state_at_charlie, expected) = store.set_up();

		let ev_map = store.0.clone();
		let fetcher = |id| ready(ev_map.get(&id).cloned());

		let exists = |id: OwnedEventId| ready(ev_map.get(&*id).is_some());

		let state_sets = [state_at_bob, state_at_charlie];
		let auth_chain: Vec<_> = state_sets
			.iter()
			.map(|map| {
				store
					.auth_event_ids(room_id(), map.values().cloned().collect())
					.unwrap()
			})
			.collect();

		let resolved =
			match super::resolve(&RoomVersionId::V2, &state_sets, &auth_chain, &fetcher, &exists)
				.await
			{
				| Ok(state) => state,
				| Err(e) => panic!("{e}"),
			};

		assert_eq!(expected, resolved);
	}

	#[tokio::test]
	async fn test_lexicographical_sort() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);

		let graph = hashmap! {
			event_id("l") => hashset![event_id("o")],
			event_id("m") => hashset![event_id("n"), event_id("o")],
			event_id("n") => hashset![event_id("o")],
			event_id("o") => hashset![], // "o" has zero outgoing edges but 4 incoming edges
			event_id("p") => hashset![event_id("o")],
		};

		let res = super::lexicographical_topological_sort(&graph, &|_id| async {
			Ok((int!(0), MilliSecondsSinceUnixEpoch(uint!(0))))
		})
		.await
		.unwrap();

		assert_eq!(
			vec!["o", "l", "n", "m", "p"],
			res.iter()
				.map(ToString::to_string)
				.map(|s| s.replace('$', "").replace(":foo", ""))
				.collect::<Vec<_>>()
		);
	}

	#[tokio::test]
	async fn ban_with_auth_chains() {
		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let ban = BAN_STATE_SET();

		let edges = vec![vec!["END", "MB", "PA", "START"], vec!["END", "IME", "MB"]]
			.into_iter()
			.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
			.collect::<Vec<_>>();

		let expected_state_ids = vec!["PA", "MB"]
			.into_iter()
			.map(event_id)
			.collect::<Vec<_>>();

		do_check(&ban.values().cloned().collect::<Vec<_>>(), edges, expected_state_ids).await;
	}

	#[tokio::test]
	async fn ban_with_auth_chains2() {
		use futures::future::ready;

		let _ = tracing::subscriber::set_default(
			tracing_subscriber::fmt().with_test_writer().finish(),
		);
		let init = INITIAL_EVENTS();
		let ban = BAN_STATE_SET();

		let mut inner = init.clone();
		inner.extend(ban);
		let store = TestStore(inner.clone());

		let state_set_a = [
			inner.get(&event_id("CREATE")).unwrap(),
			inner.get(&event_id("IJR")).unwrap(),
			inner.get(&event_id("IMA")).unwrap(),
			inner.get(&event_id("IMB")).unwrap(),
			inner.get(&event_id("IMC")).unwrap(),
			inner.get(&event_id("MB")).unwrap(),
			inner.get(&event_id("PA")).unwrap(),
		]
		.iter()
		.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.event_id.clone()))
		.collect::<StateMap<_>>();

		let state_set_b = [
			inner.get(&event_id("CREATE")).unwrap(),
			inner.get(&event_id("IJR")).unwrap(),
			inner.get(&event_id("IMA")).unwrap(),
			inner.get(&event_id("IMB")).unwrap(),
			inner.get(&event_id("IMC")).unwrap(),
			inner.get(&event_id("IME")).unwrap(),
			inner.get(&event_id("PA")).unwrap(),
		]
		.iter()
		.map(|ev| (ev.event_type().with_state_key(ev.state_key().unwrap()), ev.event_id.clone()))
		.collect::<StateMap<_>>();

		let ev_map = &store.0;
		let state_sets = [state_set_a, state_set_b];
		let auth_chain: Vec<_> = state_sets
			.iter()
			.map(|map| {
				store
					.auth_event_ids(room_id(), map.values().cloned().collect())
					.unwrap()
			})
			.collect();

		let fetcher = |id: OwnedEventId| ready(ev_map.get(&id).cloned());
		let exists = |id: OwnedEventId| ready(ev_map.get(&id).is_some());
		let resolved =
			match super::resolve(&RoomVersionId::V6, &state_sets, &auth_chain, &fetcher, &exists)
				.await
			{
				| Ok(state) => state,
				| Err(e) => panic!("{e}"),
			};

		debug!(
			resolved = ?resolved
				.iter()
				.map(|((ty, key), id)| format!("(({ty}{key:?}), {id})"))
				.collect::<Vec<_>>(),
				"resolved state",
		);

		let expected = [
			"$CREATE:foo",
			"$IJR:foo",
			"$PA:foo",
			"$IMA:foo",
			"$IMB:foo",
			"$IMC:foo",
			"$MB:foo",
		];

		for id in expected.iter().map(|i| event_id(i)) {
			// make sure our resolved events are equal to the expected list
			assert!(resolved.values().any(|eid| eid == &id) || init.contains_key(&id), "{id}");
		}
		assert_eq!(expected.len(), resolved.len());
	}

	#[tokio::test]
	async fn join_rule_with_auth_chain() {
		let join_rule = JOIN_RULE();

		let edges = vec![vec!["END", "JR", "START"], vec!["END", "IMZ", "START"]]
			.into_iter()
			.map(|list| list.into_iter().map(event_id).collect::<Vec<_>>())
			.collect::<Vec<_>>();

		let expected_state_ids = vec!["JR"].into_iter().map(event_id).collect::<Vec<_>>();

		do_check(&join_rule.values().cloned().collect::<Vec<_>>(), edges, expected_state_ids)
			.await;
	}

	#[allow(non_snake_case)]
	fn BAN_STATE_SET() -> HashMap<OwnedEventId, PduEvent> {
		vec![
			to_pdu_event(
				"PA",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
				&["CREATE", "IMA", "IPOWER"], // auth_events
				&["START"],                   // prev_events
			),
			to_pdu_event(
				"PB",
				alice(),
				TimelineEventType::RoomPowerLevels,
				Some(""),
				to_raw_json_value(&json!({ "users": { alice(): 100, bob(): 50 } })).unwrap(),
				&["CREATE", "IMA", "IPOWER"],
				&["END"],
			),
			to_pdu_event(
				"MB",
				alice(),
				TimelineEventType::RoomMember,
				Some(ella().as_str()),
				member_content_ban(),
				&["CREATE", "IMA", "PB"],
				&["PA"],
			),
			to_pdu_event(
				"IME",
				ella(),
				TimelineEventType::RoomMember,
				Some(ella().as_str()),
				member_content_join(),
				&["CREATE", "IJR", "PA"],
				&["MB"],
			),
		]
		.into_iter()
		.map(|ev| (ev.event_id.clone(), ev))
		.collect()
	}

	#[allow(non_snake_case)]
	fn JOIN_RULE() -> HashMap<OwnedEventId, PduEvent> {
		vec![
			to_pdu_event(
				"JR",
				alice(),
				TimelineEventType::RoomJoinRules,
				Some(""),
				to_raw_json_value(&json!({ "join_rule": "invite" })).unwrap(),
				&["CREATE", "IMA", "IPOWER"],
				&["START"],
			),
			to_pdu_event(
				"IMZ",
				zara(),
				TimelineEventType::RoomPowerLevels,
				Some(zara().as_str()),
				member_content_join(),
				&["CREATE", "JR", "IPOWER"],
				&["START"],
			),
		]
		.into_iter()
		.map(|ev| (ev.event_id.clone(), ev))
		.collect()
	}

	macro_rules! state_set {
        ($($kind:expr_2021 => $key:expr_2021 => $id:expr_2021),* $(,)?) => {{
            #[allow(unused_mut)]
            let mut x = StateMap::new();
            $(
                x.insert(($kind, $key.into()), $id);
            )*
            x
        }};
    }

	#[test]
	fn separate_unique_conflicted() {
		let (unconflicted, conflicted) = super::separate(
			[
				state_set![StateEventType::RoomMember => "@a:hs1" => 0],
				state_set![StateEventType::RoomMember => "@b:hs1" => 1],
				state_set![StateEventType::RoomMember => "@c:hs1" => 2],
			]
			.iter(),
		);

		assert_eq!(unconflicted, StateMap::new());
		assert_eq!(conflicted, state_set![
			StateEventType::RoomMember => "@a:hs1" => vec![0],
			StateEventType::RoomMember => "@b:hs1" => vec![1],
			StateEventType::RoomMember => "@c:hs1" => vec![2],
		],);
	}

	#[test]
	fn separate_conflicted() {
		let (unconflicted, mut conflicted) = super::separate(
			[
				state_set![StateEventType::RoomMember => "@a:hs1" => 0],
				state_set![StateEventType::RoomMember => "@a:hs1" => 1],
				state_set![StateEventType::RoomMember => "@a:hs1" => 2],
			]
			.iter(),
		);

		// HashMap iteration order is random, so sort this before asserting on it
		for v in conflicted.values_mut() {
			v.sort_unstable();
		}

		assert_eq!(unconflicted, StateMap::new());
		assert_eq!(conflicted, state_set![
			StateEventType::RoomMember => "@a:hs1" => vec![0, 1, 2],
		],);
	}

	#[test]
	fn separate_unconflicted() {
		let (unconflicted, conflicted) = super::separate(
			[
				state_set![StateEventType::RoomMember => "@a:hs1" => 0],
				state_set![StateEventType::RoomMember => "@a:hs1" => 0],
				state_set![StateEventType::RoomMember => "@a:hs1" => 0],
			]
			.iter(),
		);

		assert_eq!(unconflicted, state_set![
			StateEventType::RoomMember => "@a:hs1" => 0,
		],);
		assert_eq!(conflicted, StateMap::new());
	}

	#[test]
	fn separate_mixed() {
		let (unconflicted, conflicted) = super::separate(
			[
				state_set![StateEventType::RoomMember => "@a:hs1" => 0],
				state_set![
					StateEventType::RoomMember => "@a:hs1" => 0,
					StateEventType::RoomMember => "@b:hs1" => 1,
				],
				state_set![
					StateEventType::RoomMember => "@a:hs1" => 0,
					StateEventType::RoomMember => "@c:hs1" => 2,
				],
			]
			.iter(),
		);

		assert_eq!(unconflicted, state_set![
			StateEventType::RoomMember => "@a:hs1" => 0,
		],);
		assert_eq!(conflicted, state_set![
			StateEventType::RoomMember => "@b:hs1" => vec![1],
			StateEventType::RoomMember => "@c:hs1" => vec![2],
		],);
	}
}
