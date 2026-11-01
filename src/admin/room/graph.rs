use std::{
	collections::{HashMap, HashSet},
	fmt::Write,
};

use clap::Subcommand;
use conduwuit::{Err, Event, PduEvent, Result, err, utils::stream::BroadbandExt};
use conduwuit_macros::admin_command_dispatch;
use futures::StreamExt;
use ruma::{EventId, OwnedEventId, OwnedRoomId, OwnedRoomOrAliasId, OwnedServerName, UInt};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum GraphCommand {
	/// Lists all forward extremities in a room.
	ListExtremities {
		room: OwnedRoomOrAliasId,
	},

	/// Fetches a PDU (event) by its event ID, as stored in the database. Also
	/// includes details about the PDU state.
	GetPDU {
		event_id: OwnedEventId,
	},

	/// Fetches a PDU over federation. Does not store the result.
	FetchPDU {
		event_id: OwnedEventId,
		server_name: OwnedServerName,
	},

	/// Yields the first PDU we became aware of in a room.
	/// This is always the room's create event for local rooms, or the first
	/// membership event that introduced us to the remote room.
	#[clap(name = "first-pdu")]
	FirstPduInRoom {
		room_id: OwnedRoomId,
	},

	/// Yields the most recent PDU in a room.
	#[clap(name = "latest-pdu")]
	LatestPduInRoom {
		room_id: OwnedRoomId,
	},

	/// Draws a mermaid graph of the auth chain for an event.
	ShowAuthChain {
		event_id: OwnedEventId,
	},
}

impl crate::Context<'_> {
	pub(super) async fn list_extremities(&self, room: OwnedRoomOrAliasId) -> Result {
		let room_id = self.services.rooms.alias.resolve(&room).await?;
		let mut extremities = self
			.services
			.rooms
			.state
			.get_forward_extremities(&room_id)
			.broad_filter_map(|event_id| async move {
				self.services.rooms.timeline.get_pdu(&event_id).await.ok()
			})
			.collect::<Vec<_>>()
			.await;
		extremities.sort_by_key(|pdu| (pdu.depth, pdu.origin_server_ts));
		if extremities.is_empty() {
			return Err!("No room extremities.");
		}
		self.write_str("| Event ID | Type | Sender | Timestamp | Depth |\n")
			.await?;
		self.write_str("| -------- | ---- | ------ | --------- | ----- |\n")
			.await?;
		for pdu in extremities {
			let url = format!(
				"https://matrix.to/#/{}/{}?via={}",
				room_id,
				pdu.event_id(),
				self.services.server.name
			);
			self.write_str(&format!(
				"| [`{}`]({url}) | `{}` | `{}` | `{}` | `{}` |\n",
				pdu.event_id(),
				pdu.kind(),
				pdu.sender(),
				pdu.origin_server_ts,
				pdu.depth
			))
			.await?;
		}

		Ok(())
	}

	pub(super) async fn get_pdu(&self, event_id: OwnedEventId) -> Result {
		let mut outlier = false;
		let mut pdu_json = self
			.services
			.rooms
			.timeline
			.get_non_outlier_pdu_json(&event_id)
			.await;

		if pdu_json.is_err() {
			outlier = true;
			pdu_json = self.services.rooms.timeline.get_pdu_json(&event_id).await;
		}
		let rejected = self
			.services
			.rooms
			.pdu_metadata
			.is_event_rejected(&event_id)
			.await;
		let soft_failed = self
			.services
			.rooms
			.pdu_metadata
			.is_event_soft_failed(&event_id)
			.await;

		match pdu_json {
			| Err(_) => return Err!("PDU not found locally."),
			| Ok(json) => {
				let text = serde_json::to_string_pretty(&json)?;
				let msg = if rejected {
					"Rejected PDU:"
				} else if soft_failed {
					"Soft-failed PDU:"
				} else if outlier {
					"Outlier PDU:"
				} else {
					"Timeline PDU:"
				};
				write!(self, "{msg}\n```json\n{text}\n```")
			},
		}
		.await
	}

	pub(super) async fn fetch_pdu(
		&self,
		event_id: OwnedEventId,
		server: OwnedServerName,
	) -> Result {
		let response = self
			.services
			.sending
			.send_federation_request(
				&server,
				ruma::api::federation::event::get_event::v1::Request::new(event_id.clone()),
			)
			.await?;
		let (real_event_id, room_id, pdu_json) = self
			.services
			.rooms
			.event_handler
			.parse_incoming_pdu(&response.pdu, None)
			.await?;
		self.write_str(&format!(
			"Event ID: {real_event_id}\nRoom ID: {room_id}\nPDU:\n```json\n{}\n```",
			serde_json::to_string_pretty(&pdu_json)?
		))
		.await
	}

	pub(super) async fn first_pdu_in_room(&self, room_id: OwnedRoomId) -> Result {
		self.bail_restricted()?;

		let first_pdu = self
			.services
			.rooms
			.timeline
			.first_pdu_in_room(&room_id)
			.await
			.map(Event::into_canonical_object)
			.and_then(|v| serde_json::to_string_pretty(&v).map_err(Into::into))
			.map_err(|e| err!(Database("Failed to find the first PDU in database: {e:?}")))?;

		let out = format!("```json\n{first_pdu}\n```");
		self.write_str(&out).await
	}

	pub(super) async fn latest_pdu_in_room(&self, room_id: OwnedRoomId) -> Result {
		self.bail_restricted()?;

		let latest_pdu = self
			.services
			.rooms
			.timeline
			.latest_pdu_in_room(&room_id)
			.await
			.map(Event::into_canonical_object)
			.and_then(|v| serde_json::to_string_pretty(&v).map_err(Into::into))
			.map_err(|e| err!(Database("Failed to find the latest PDU in database: {e:?}")))?;

		let out = format!("```json\n{latest_pdu}\n```");
		self.write_str(&out).await
	}

	pub(super) async fn show_auth_chain(&self, event_id: OwnedEventId) -> Result {
		// TODO: This command sucks and really needs to elide redundant auth
		// edges.
		let node_status = async |event_id: &EventId, missing: bool| -> NodeStatus {
			if self
				.services
				.rooms
				.pdu_metadata
				.is_event_rejected(event_id)
				.await
			{
				NodeStatus::Rejected(missing)
			} else if self
				.services
				.rooms
				.pdu_metadata
				.is_event_soft_failed(event_id)
				.await
			{
				NodeStatus::SoftFailed(missing)
			} else {
				NodeStatus::Normal(missing)
			}
		};

		let Ok(root) = self.services.rooms.timeline.get_pdu(&event_id).await else {
			return Err!("Event not found.");
		};

		let mut graph = String::from(
			"```mermaid\n%% This is a mermaid graph. You can plug this output into\n\
		%% https://mermaid.live/edit to visualise it on-the-fly.\nflowchart TD\n\
		classDef rejected fill:#ffe5e5,stroke:#cc0000,stroke-width:2px,color:#000;\n\
		classDef soft_failed fill:#fff6cc,stroke:#c9a400,stroke-width:2px,color:#000;\n"
		);

		let mut node_ids: HashMap<OwnedEventId, String> = HashMap::new();
		let mut cached_events: HashMap<OwnedEventId, PduEvent> =
			HashMap::from([(event_id.clone(), root.clone())]);
		let mut scheduled: HashSet<OwnedEventId> = HashSet::from([event_id.clone()]);
		let mut visited: HashSet<OwnedEventId> = HashSet::new();
		let mut stack = vec![root];
		let mut next_node_id = 0_usize;

		let node_id_for = |event_id: &OwnedEventId,
		                   node_ids: &mut HashMap<OwnedEventId, String>,
		                   next_node_id: &mut usize| {
			node_ids
				.entry(event_id.clone())
				.or_insert_with(|| {
					let id = format!("n{}", *next_node_id);
					*next_node_id = next_node_id.saturating_add(1);
					id
				})
				.clone()
		};
		let node_name = |e: &PduEvent| {
			if let Some(state_key) = e.state_key() {
				format!("{},'{}'", e.event_type(), state_key)
			} else {
				format!("{}", e.event_type())
			}
		};

		while let Some(event) = stack.pop() {
			let current_event_id = event.event_id().to_owned();
			if !visited.insert(current_event_id.clone()) {
				continue;
			}

			let current_node_id =
				node_id_for(&current_event_id, &mut node_ids, &mut next_node_id);
			let current_status = node_status(&current_event_id, false).await;

			render_node(
				&mut graph,
				&current_node_id,
				&current_event_id,
				&node_name(&event),
				current_status,
			)?;

			let mut children = Vec::with_capacity(event.auth_events.len());
			for auth_event_id in event.auth_events().rev() {
				let auth_event_id = auth_event_id.to_owned();
				let auth_node_id = node_id_for(&auth_event_id, &mut node_ids, &mut next_node_id);
				writeln!(graph, "{current_node_id} --> {auth_node_id}")?;

				let first_seen = scheduled.insert(auth_event_id.clone());
				let auth_pdu = if let Some(auth_pdu) = cached_events.get(&auth_event_id) {
					// NOTE: events might be referenced multiple times (like the
					// create event) so this saves some cheeky db lookup
					// time
					Some(auth_pdu.clone())
				} else if first_seen {
					match self.services.rooms.timeline.get_pdu(&auth_event_id).await {
						| Ok(auth_event) => {
							cached_events.insert(auth_event_id.clone(), auth_event.clone());
							Some(auth_event)
						},
						| Err(_) => None,
					}
				} else {
					None
				};

				// NOTE: Depth is used as the primary sorting key here, even
				// though it has no bearing on state resolution or anything.
				// Timestamp is used as a tiebreaker, failing back to
				// lexicographical comparison.
				let (depth, ts) = auth_pdu
					.as_ref()
					.map_or((UInt::MAX, UInt::MAX), |pdu| (pdu.depth, pdu.origin_server_ts));

				children.push(AuthChild {
					node_id: auth_node_id,
					event_id: auth_event_id,
					depth,
					ts,
					first_seen,
					pdu: auth_pdu,
				});
			}

			children.sort_by(|a, b| {
				a.depth
					.cmp(&b.depth)
					.then(a.ts.cmp(&b.ts))
					.then(a.event_id.as_str().cmp(b.event_id.as_str()))
			});

			for child in children.into_iter().rev() {
				if !child.first_seen {
					continue;
				}

				if let Some(child_pdu) = child.pdu {
					// We have this PDU so will want to traverse it.
					stack.push(child_pdu);
				} else {
					// We don't have this PDU locally so we can't traverse its
					// auth events, but we can still render it as a node.
					render_node(
						&mut graph,
						&child.node_id,
						&child.event_id,
						"",
						node_status(&child.event_id, true).await,
					)?;
				}
			}
		}

		graph.push_str("```\n");
		self.write_str(&graph).await
	}
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum NodeStatus {
	Normal(bool),
	SoftFailed(bool),
	Rejected(bool),
}

struct AuthChild {
	node_id: String,
	event_id: OwnedEventId,
	depth: UInt,
	ts: UInt,
	first_seen: bool,
	pdu: Option<PduEvent>,
}

fn render_node(
	graph: &mut String,
	node_id: &str,
	event_id: &EventId,
	name: &str,
	status: NodeStatus,
) -> Result {
	let evt_str = event_id.to_string();

	let status_label = match status {
		| NodeStatus::Normal(false) => format!("{evt_str}: {name}"),
		| NodeStatus::Normal(true) => format!("{evt_str}: {name} (missing locally)"),
		| NodeStatus::SoftFailed(false) => format!("{evt_str}: {name} (soft-failed)"),
		| NodeStatus::SoftFailed(true) =>
			format!("{evt_str}: {name} (soft-failed & missing locally)"),
		| NodeStatus::Rejected(false) => format!("{evt_str}: {name} (rejected)"),
		| NodeStatus::Rejected(true) => format!("{evt_str}: {name} (rejected & missing locally)"),
	};

	writeln!(graph, "{node_id}[\"{}\"]", status_label.as_str())?;

	match status {
		| NodeStatus::Rejected(_) => writeln!(graph, "class {node_id} rejected;")?,
		| NodeStatus::SoftFailed(_) => writeln!(graph, "class {node_id} soft_failed;")?,
		| NodeStatus::Normal(_) => {},
	}

	Ok(())
}
