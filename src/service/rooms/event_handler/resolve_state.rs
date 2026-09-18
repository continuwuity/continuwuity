use std::{
	borrow::Borrow,
	collections::{HashMap, HashSet, VecDeque},
	sync::Arc,
};

use conduwuit::{
	Error, Result, err,
	state_res::{self, StateMap},
	trace,
	utils::stream::{IterStream, ReadyExt, TryWidebandExt, WidebandExt},
	warn,
};
use futures::{FutureExt, StreamExt, TryFutureExt, TryStreamExt, future::try_join};
use itertools::cloned;
use ruma::{
	EventId, OwnedEventId, RoomId,
	room_version_rules::RoomVersionRules,
	state_res::{Event, utils::event_id_set::EventIdSet},
};

use crate::rooms::state_compressor::CompressedState;

impl super::Service {
	/// Resolves the state based on the incoming fork states with the given room
	/// version rules.
	pub async fn resolve_state(
		&self,
		room_id: &RoomId,
		room_version_rules: &RoomVersionRules,
		incoming_state: HashMap<u64, OwnedEventId>,
	) -> Result<Arc<CompressedState>> {
		trace!("Loading current room state ids");
		let current_sstatehash = self
			.services
			.state
			.get_room_shortstatehash(room_id)
			.map_err(|e| err!(Database(error!("No state for {room_id:?}: {e:?}"))))
			.await?;

		let current_state_ids: HashMap<_, _> = self
			.services
			.state_accessor
			.state_full_ids(current_sstatehash)
			.collect()
			.await;

		trace!("Loading fork states");
		let fork_states = [current_state_ids, incoming_state];
		let auth_chain_sets = fork_states
			.iter()
			.try_stream()
			.wide_and_then(|state| {
				self.services
					.auth_chain
					.event_ids_iter(room_id, state.values().map(Borrow::borrow))
					.try_collect()
			})
			.try_collect::<Vec<EventIdSet<OwnedEventId>>>();

		let fork_states = fork_states
			.iter()
			.stream()
			.wide_then(|fork_state| {
				let shortstatekeys = fork_state.keys().copied().stream();
				let event_ids = fork_state.values().cloned().stream();
				self.services
					.short
					.multi_get_statekey_from_short(shortstatekeys)
					.zip(event_ids)
					.ready_filter_map(|(ty_sk, id)| {
						Some((ty_sk.ok().map(|(k, s)| (k, s.to_string()))?, id))
					})
					.collect()
			})
			.map(Ok::<_, Error>)
			.try_collect::<Vec<ruma::state_res::StateMap<OwnedEventId>>>();

		let (fork_states, auth_chain_sets) = try_join(fork_states, auth_chain_sets).await?;

		trace!("Resolving state");
		let state =
			self.state_resolution(room_version_rules, fork_states.as_slice(), auth_chain_sets)?;

		trace!("State resolution done.");
		let state_events: Vec<_> = state
			.iter()
			.stream()
			.wide_then(|((event_type, state_key), event_id)| {
				self.services
					.short
					.get_or_create_shortstatekey(event_type, state_key)
					.map(move |shortstatekey| (shortstatekey, event_id))
			})
			.collect()
			.await;

		trace!("Compressing state...");
		let new_room_state: CompressedState = self
			.services
			.state_compressor
			.compress_state_events(state_events.iter().map(|(ssk, eid)| (ssk, (*eid).borrow())))
			.collect()
			.await;

		Ok(Arc::new(new_room_state))
	}

	/// Actually performs state resolution from the given state sets, auth chain
	/// sets, and room version rules.
	pub fn state_resolution<'a>(
		&'a self,
		room_version_rules: &'a RoomVersionRules,
		state_sets: &[ruma::state_res::StateMap<OwnedEventId>],
		auth_chain_sets: Vec<EventIdSet<OwnedEventId>>,
	) -> Result<ruma::state_res::StateMap<OwnedEventId>> {
		let event_fetch = |event_id| self.event_fetch(event_id);
		let calc_subgraph = |ss| self.calculate_conflicted_state_subgraph(ss);
		ruma::state_res::resolve(
			&room_version_rules.authorization,
			&room_version_rules
				.state_res
				.v2_rules()
				.expect("can only use state resolution v2 rules"),
			state_sets,
			auth_chain_sets,
			event_fetch,
			calc_subgraph,
		)
		.map_err(|e| err!(error!("state resolution failed: {e:?}")))
	}

	/// Calculates the conflicted state subgraph for state resolution.
	fn calculate_conflicted_state_subgraph<'a>(
		&self,
		state_set: &ruma::state_res::StateMap<&'a EventId>,
	) -> Result<HashSet<&'a EventId>> {
		let conflicted_events: HashSet<_> = state_set.values().cloned().collect();
		let mut subgraph: HashSet<_> = HashSet::new();
		let mut stack: Vec<Vec<_>> = vec![conflicted_events.iter().cloned().collect::<Vec<_>>()];
		let mut path: Vec<_> = Vec::new();
		let mut seen: HashSet<_> = HashSet::new();
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
			let Some(evt) = self.event_fetch(event_id.clone()) else {
				warn!("could not fetch event {} to calculate conflicted subgraph", event_id);
				path.pop();
				continue;
			};
			stack.push(evt.auth_events().collect());
			seen.insert(event_id);
		}
		Ok(subgraph)
	}
}
