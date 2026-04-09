use std::{collections::HashSet, sync::Arc};

use conduwuit::{
	Err, Event, Result, info,
	utils::{IterStream, ReadyExt, TryFutureExtExt, stream::BroadbandExt},
	warn,
};
use futures::{FutureExt, StreamExt, TryFutureExt};
use ruma::{
	OwnedEventId, OwnedRoomId, OwnedServerName, RoomId, ServerName, UInt, UserId,
	api::{
		client::space::SpaceHierarchyRoomsChunk,
		federation::space::{SpaceHierarchyParentSummary, get_hierarchy},
	},
	assign,
	events::{
		StateEventType,
		space::child::{HierarchySpaceChildEvent, SpaceChildEventContent},
	},
	room::{JoinRuleSummary, RestrictedSummary, RoomSummary},
	serde::Raw,
};

use crate::{Dep, rooms, sending};

pub struct Service {
	services: Services,
}

struct Services {
	event_handler: Dep<rooms::event_handler::Service>,
	metadata: Dep<rooms::metadata::Service>,
	sending: Dep<sending::Service>,
	state: Dep<rooms::state::Service>,
	state_accessor: Dep<rooms::state_accessor::Service>,
	state_cache: Dep<rooms::state_cache::Service>,
	timeline: Dep<rooms::timeline::Service>,
}

pub enum Accessibility<T> {
	Accessible(T),
	Inaccessible,
	NotFound,
}

struct SpaceSummaryAndChildren {
	/// The summary of the space.
	summary: SpaceHierarchyRoomsChunk,
	/// All child rooms of the space.
	children: Vec<SpaceChild>,
	/// Child rooms of the space which are not accessible to the local server.
	inaccessible_children: Vec<OwnedRoomId>,
}

struct SpaceChild {
	room_id: OwnedRoomId,
	via: Vec<OwnedServerName>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				event_handler: args
					.depend::<rooms::event_handler::Service>("rooms::event_handler"),
				metadata: args.depend::<rooms::metadata::Service>("rooms::metadata"),
				sending: args.depend::<sending::Service>("sending"),
				state: args.depend::<rooms::state::Service>("rooms::state"),
				state_accessor: args
					.depend::<rooms::state_accessor::Service>("rooms::state_accessor"),
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Summarize a room for a local user, possibly by querying over federation
	/// if we don't have the room locally.
	pub async fn get_room_summary_for_user(
		&self,
		querying_user: Option<&UserId>,
		room_id: &RoomId,
		via: &[OwnedServerName],
	) -> Result<Accessibility<RoomSummary>> {
		let summary = {
			if let Some(summary) = self.build_local_room_summary(room_id).await {
				// We have this room locally.

				summary
			} else if let Some((SpaceHierarchyParentSummary { summary, .. }, _)) =
				self.fetch_remote_summary(room_id, via, false).await?
			{
				// A via has this room.

				summary
			} else {
				// We don't have this room and none of the vias have it either.

				return Ok(Accessibility::NotFound);
			}
		};

		// Check if the room is visible to the querying user.
		if !self.user_may_see_summary(querying_user, &summary).await {
			return Ok(Accessibility::Inaccessible);
		}

		Ok(Accessibility::Accessible(summary))
	}

	/// Fetch information about a room and its children, possibly by querying
	/// over federation if we don't have the room locally.
	///
	/// This is similar to [`Self::get_room_summary_for_user`] but includes
	/// additional data which is needed to traverse the room hierarchy.
	async fn get_room_summary_and_children_for_user(
		&self,
		querying_user: Option<&UserId>,
		room_id: &RoomId,
		via: Option<&[OwnedServerName]>,
		suggested_only: bool,
	) -> Result<Accessibility<SpaceSummaryAndChildren>> {
		let (summary, inaccessible_children) = {
			if let Some(summary) = self.build_local_room_summary(room_id).await {
				// We have this room locally.
				let children_state = self.get_space_child_events(room_id).await;

				// All of the room's children are accessible to this server (because we have the
				// full room and its state), although some of them may not be accessible to
				// the querying user.
				(SpaceHierarchyRoomsChunk::new(summary, children_state), vec![])
			} else if let Some(via) = via
				&& let Some((
					SpaceHierarchyParentSummary { summary, children_state, .. },
					inaccessible_children,
				)) = self
					.fetch_remote_summary(room_id, via, suggested_only)
					.await?
			{
				// A via has this room.

				(SpaceHierarchyRoomsChunk::new(summary, children_state), inaccessible_children)
			} else {
				// We don't have this room and none of the vias have it either.

				return Ok(Accessibility::NotFound);
			}
		};

		// Check if the room is visible to the querying user.
		if !self
			.user_may_see_summary(querying_user, &summary.summary)
			.await
		{
			return Ok(Accessibility::Inaccessible);
		}

		let children = summary
            .children_state
            .iter()
            // Ignore deserialization failures
            .flat_map(Raw::deserialize)
            // Filter out non-suggested children if suggested_only is set
            .filter(|child| !suggested_only || child.content.suggested)
            .map(|child| SpaceChild { room_id: child.state_key, via: child.content.via })
            .collect();

		Ok(Accessibility::Accessible(SpaceSummaryAndChildren {
			summary,
			children,
			inaccessible_children,
		}))
	}

	/// Summarize a room and its children for a local user, possibly by querying
	/// over federation if we don't have the space locally.
	pub async fn get_room_hierarchy_for_user(
		&self,
		querying_user: &UserId,
		room_id: OwnedRoomId,
		max_depth: Option<UInt>,
		suggested_only: bool,
	) -> Result<Accessibility<Vec<SpaceHierarchyRoomsChunk>>> {
		// This function traverses the space hierarchy tree depth-first as required by
		// the specification.

		// Check accessibility of the root room first, because we need to error out
		// if it isn't accessible.
		// TODO refactor this once the Try trait is stable
		let root_summary = match self
			.get_room_summary_and_children_for_user(
				Some(querying_user),
				&room_id,
				// Clients can't specify vias for the root room
				None,
				suggested_only,
			)
			.await?
		{
			| Accessibility::Accessible(root_summary) => root_summary,
			| Accessibility::Inaccessible => return Ok(Accessibility::Inaccessible),
			| Accessibility::NotFound => return Ok(Accessibility::NotFound),
		};

		let mut queue = vec![root_summary.children];
		let mut summaries = vec![root_summary.summary];
		let mut inaccessible_children: HashSet<_> =
			root_summary.inaccessible_children.into_iter().collect();

		// TODO refactor this with Vec::peek_mut once it's stabilized
		loop {
			let Some(layer) = queue.last_mut() else {
				break;
			};

			let Some(SpaceChild { room_id, via }) = layer.pop() else {
				// If this layer is empty, discard it from the queue and continue
				queue.pop();
				continue;
			};

			// Do not request rooms which have been determined to be inaccessible
			if inaccessible_children.contains(&room_id) {
				continue;
			}

			let summary = match self
				.get_room_summary_and_children_for_user(
					Some(querying_user),
					&room_id,
					Some(&via),
					suggested_only,
				)
				.await
			{
				| Ok(Accessibility::Accessible(summary)) => summary,
				| Ok(Accessibility::Inaccessible) => {
					// Mark this room as inaccessible and skip it
					inaccessible_children.insert(room_id);
					continue;
				},
				| Ok(Accessibility::NotFound) => {
					// Skip children which we can't find
					continue;
				},
				| Err(_) => {
					// Skip children which we failed to fetch over federation
					continue;
				},
			};

			summaries.push(summary.summary);
			inaccessible_children.extend(summary.inaccessible_children.into_iter());

			// Don't traverse the tree deeper than max_depth
			#[allow(
				clippy::as_conversions,
				clippy::arithmetic_side_effects,
				reason = "queue.len() should never be large enough to cause strange behavior \
				          here"
			)]
			if max_depth.is_some_and(|max_depth| (queue.len() as u64 + 1) > max_depth.into()) {
				continue;
			}

			// Add accessible children as a new layer
			if !summary.children.is_empty() {
				queue.push(summary.children);
			}
		}

		Ok(Accessibility::Accessible(summaries))
	}

	/// Summarize a _local_ room and its children for a remote server.
	pub async fn get_local_room_summary_for_server(
		&self,
		querying_server: &ServerName,
		room_id: &RoomId,
		suggested_only: bool,
	) -> Accessibility<get_hierarchy::v1::Response> {
		let Some(summary) = self.build_local_room_summary(room_id).await else {
			return Accessibility::NotFound;
		};

		// Check if the server can see the root room's summary
		if !self.server_may_see_summary(querying_server, &summary).await {
			return Accessibility::Inaccessible;
		}

		let children_state = self.get_space_child_events(room_id).await;

		let (accessible_children, inaccessible_children) = children_state
            .iter()
            // Ignore deserialization failures
            .flat_map(Raw::deserialize)
            // Filter out non-suggested children if suggested_only is set
            .filter(|child| !suggested_only || child.content.suggested)
            // Fetch summaries for the children in parallel
            .stream()
            .broad_then(async |child| {
                let summary = {
                    if let Some(summary) = self.build_local_room_summary(&child.state_key).await {
                        if self.server_may_see_summary(querying_server, &summary).await {
                            Accessibility::Accessible(summary)
                        } else {
                            Accessibility::Inaccessible
                        }
                    } else {
                        Accessibility::NotFound
                    }
                };

                (child.state_key, summary)
            })
            // Sort the children into two Vecs by accessibility
            .ready_fold_default(|(mut accessible_children, mut inaccessible_children): (Vec<_>, Vec<_>), (room_id, summary)| {
                match summary {
                    Accessibility::Accessible(summary) => {
                        accessible_children.push(summary);
                    },
                    Accessibility::Inaccessible => {
                        inaccessible_children.push(room_id);
                    },
                    Accessibility::NotFound => {
                        // Skip inaccessible children
                    }
                }

                (accessible_children, inaccessible_children)
            })
            .await;

		Accessibility::Accessible(assign!(
			get_hierarchy::v1::Response::new(SpaceHierarchyParentSummary::new(summary, children_state)),
			{ children: accessible_children, inaccessible_children: inaccessible_children }
		))
	}

	/// Prepare a summary of a room known to this server.
	async fn build_local_room_summary(&self, room_id: &RoomId) -> Option<RoomSummary> {
		// If we can't find a version for this room, it doesn't exist.
		let room_version = self.services.state.get_room_version(room_id).await.ok()?;

		let (
			join_rule,
			guest_can_join,
			num_joined_members,
			world_readable,
			canonical_alias,
			name,
			topic,
			avatar_url,
			room_type,
			encryption,
		) = async {
			futures::join!(
				self.services.state_accessor.get_join_rules(room_id),
				self.services.state_accessor.guest_can_join(room_id),
				self.services
					.state_cache
					.room_joined_count(room_id)
					.unwrap_or(0),
				self.services.state_accessor.is_world_readable(room_id),
				self.services
					.state_accessor
					.get_canonical_alias(room_id)
					.ok(),
				self.services.state_accessor.get_name(room_id).ok(),
				self.services.state_accessor.get_room_topic(room_id).ok(),
				self.services
					.state_accessor
					.get_avatar(room_id)
					.map(|res| res.into_option().unwrap_or_default().url),
				self.services.state_accessor.get_room_type(room_id).ok(),
				self.services
					.state_accessor
					.get_room_encryption(room_id)
					.ok(),
			)
		}
		.boxed()
		.await;

		let summary = assign!(
			RoomSummary::new(
				room_id.to_owned(),
				join_rule.into(),
				guest_can_join,
				num_joined_members.try_into().expect("number of joined members should fit into a UInt"),
				world_readable,
			),
			{
				canonical_alias: canonical_alias,
				name: name,
				topic: topic,
				avatar_url: avatar_url,
				room_type: room_type,
				encryption: encryption,
				room_version: Some(room_version),
			}
		);

		Some(summary)
	}

	/// Query remote servers for the summary of a room.
	async fn fetch_remote_summary(
		&self,
		room_id: &RoomId,
		via: &[OwnedServerName],
		suggested_only: bool,
	) -> Result<Option<(SpaceHierarchyParentSummary, Vec<OwnedRoomId>)>> {
		if self.services.metadata.is_disabled(room_id).await {
			return Err!(Request(Forbidden("This room is blocked by this server.")));
		}

		if via.is_empty() {
			return Err!(Request(MissingParam(
				"No servers were provided with which to query this room's summary."
			)));
		}

		let request = assign!(get_hierarchy::v1::Request::new(room_id.to_owned()), { suggested_only: suggested_only });

		for server in via {
			info!(%room_id, %server, "Asking for room summary over federation");

			match self
				.services
				.sending
				.send_federation_request(server, request.clone())
				.await
			{
				| Ok(get_hierarchy::v1::Response { room, inaccessible_children, .. }) => {
					if room.summary.room_id != room_id {
						warn!(
							%server,
							expected_room = %room_id,
							returned_room = %room.summary.room_id,
							"Server didn't return the room we asked for"
						);
						continue;
					}

					return Ok(Some((room, inaccessible_children)));
				},
				| Err(err) => {
					info!(%room_id, %server, %err, "Server could not provide a summary for this room");
				},
			}
		}

		info!(%room_id, "No servers queried could provide a summary for this room");
		Ok(None)
	}

	/// Get the stripped m.space.child events of a room.
	async fn get_space_child_events(
		&self,
		room_id: &RoomId,
	) -> Vec<Raw<HierarchySpaceChildEvent>> {
		let current_shortstatehash = self
			.services
			.state
			.get_room_shortstatehash(room_id)
			.await
			.expect("room should have a current state");

		self.services
			.state_accessor
			.state_keys_with_ids(current_shortstatehash, &StateEventType::SpaceChild)
			.broad_filter_map(move |(state_key, event_id): (_, OwnedEventId)| async move {
				self.services
					.timeline
					.get_pdu(&event_id)
					.map_ok(move |pdu| (state_key, pdu))
					.ok()
					.await
			})
			.ready_filter_map(move |(state_key, pdu)| {
				let Ok(content) = pdu.get_content::<SpaceChildEventContent>() else {
					return None;
				};

				if content.via.is_empty() {
					return None;
				}

				if RoomId::parse(&state_key).is_err() {
					return None;
				}

				Some(pdu.into_format())
			})
			.collect()
			.await
	}

	/// Determine if a user (possibly anonymous) may view a room summary.
	async fn user_may_see_summary(
		&self,
		querying_user: Option<&UserId>,
		summary: &RoomSummary,
	) -> bool {
		// Anyone can view the summary of world-readable rooms.
		if summary.world_readable {
			return true;
		}

		// If the user is joined or invited they may always view the summary.
		if let Some(querying_user) = querying_user
			&& (self
				.services
				.state_cache
				.is_joined(querying_user, &summary.room_id)
				.await || self
				.services
				.state_cache
				.is_invited(querying_user, &summary.room_id)
				.await)
		{
			return true;
		}

		// Otherwise, visibility depends on the join rule.
		match (&summary.join_rule, querying_user) {
			// Anyone can view summaries for `public`, `knock`, and `knock_restricted` rooms.
			| (
				JoinRuleSummary::Public
				| JoinRuleSummary::Knock
				| JoinRuleSummary::KnockRestricted(_),
				_,
			) => true,

			// The user may be able to view the summary for a `restricted` room, even if they
			// aren't invited, provided they're in one of the allowed rooms.
			| (
				JoinRuleSummary::Restricted(RestrictedSummary { allowed_room_ids, .. }),
				Some(querying_user),
			) =>
				self.services
					.state_cache
					.rooms_joined(querying_user)
					.ready_any(|room_id| allowed_room_ids.contains(&room_id))
					.await,

			// In all other cases, the user may not view the summary.
			| _ => false,
		}
	}

	// Determine if a remote server may view a room summary.
	async fn server_may_see_summary(
		&self,
		querying_server: &ServerName,
		summary: &RoomSummary,
	) -> bool {
		// Servers may not see summaries of rooms they're ACLed from.
		if self
			.services
			.event_handler
			.acl_check(querying_server, &summary.room_id)
			.await
			.is_err()
		{
			return false;
		}

		// Servers may always see summaries if any of their users are participating in
		// the room. It's the requesting server's job to restrict visibility on a
		// per-user basis.
		if self
			.services
			.state_cache
			.server_in_room(querying_server, &summary.room_id)
			.await
		{
			return true;
		}

		// If the server isn't in the room, the same visibility rules apply as for
		// anonymous summary requests.
		self.user_may_see_summary(None, summary).await
	}
}
