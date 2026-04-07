use std::{collections::HashSet, iter::once};

use conduwuit::{Err, PduEvent};
use conduwuit_core::{
	Result, debug, debug_warn, err, implement, info,
	matrix::{
		event::Event,
		pdu::{PduCount, PduId, RawPduId},
	},
	utils::{IterStream, ReadyExt},
	validated, warn,
};
use futures::{FutureExt, Stream, StreamExt};
use ruma::{
	CanonicalJsonObject, EventId, Int, OwnedServerName, RoomId, ServerName,
	api::federation,
	events::{
		StateEventType, TimelineEventType,
		room::{
			create::RoomCreateEventContent,
			power_levels::{RoomPowerLevelsEventContent, UserPowerLevel},
		},
	},
	uint,
};
use serde_json::value::RawValue as RawJsonValue;

use super::ExtractBody;

#[implement(super::Service)]
#[tracing::instrument(name = "backfill", level = "trace", skip(self))]
pub async fn backfill_if_required(&self, room_id: &RoomId, from: PduCount) -> Result<()> {
	if self
		.services
		.state_cache
		.room_joined_count(room_id)
		.await
		.is_ok_and(|count| count <= 1)
		&& !self
			.services
			.state_accessor
			.is_world_readable(room_id)
			.await
	{
		// Room is empty (1 user or none), there is no one that can backfill
		debug_warn!("Room {room_id} is empty, skipping backfill");
		return Ok(());
	}

	let first_pdu = self
		.first_item_in_room(room_id)
		.await
		.expect("Room is not empty");

	if first_pdu.0 < from {
		// No backfill required, there are still events between them
		debug!("No backfill required in room {room_id}, {:?} < {from}", first_pdu.0);
		return Ok(());
	}

	let mut servers = self.candidate_backfill_servers(room_id).await;

	let mut federated_room = false;

	for backfill_server in servers {
		if !self.services.globals.server_is_ours(&backfill_server) {
			federated_room = true;
		}
		info!("Asking {backfill_server} for backfill in {room_id}");
		let response = self
			.services
			.sending
			.send_federation_request(
				&backfill_server,
				federation::backfill::get_backfill::v1::Request::new(
					room_id.to_owned(),
					vec![first_pdu.1.event_id().to_owned()],
					uint!(100),
				),
			)
			.await;
		match response {
			| Ok(response) => {
				for pdu in response.pdus {
					if let Err(e) = self.backfill_pdu(&backfill_server, pdu).boxed().await {
						debug_warn!("Failed to add backfilled pdu in room {room_id}: {e}");
					}
				}
				return Ok(());
			},
			| Err(e) => {
				warn!("{backfill_server} failed to provide backfill for room {room_id}: {e}");
			},
		}
	}

	if federated_room {
		warn!("No servers could backfill, but backfill was needed in room {room_id}");
	}
	Ok(())
}

#[implement(super::Service)]
#[tracing::instrument(name = "get_remote_pdu", level = "debug", skip(self))]
pub async fn get_remote_pdu(&self, room_id: &RoomId, event_id: &EventId) -> Result<PduEvent> {
	let local = self.get_pdu(event_id).await;
	if local.is_ok() {
		// We already have this PDU, no need to backfill
		debug!("We already have {event_id} in {room_id}, no need to backfill.");
		return local;
	}
	debug!("Preparing to fetch event {event_id} in room {room_id} from remote servers.");
	// Similar to backfill_if_required, but only for a single PDU
	// Fetch a list of servers to try
	if self
		.services
		.state_cache
		.room_joined_count(room_id)
		.await
		.is_ok_and(|count| count <= 1)
		&& !self
			.services
			.state_accessor
			.is_world_readable(room_id)
			.await
	{
		// Room is empty (1 user or none), there is no one that can backfill
		return Err!(Request(NotFound("No one can backfill this PDU, room is empty.")));
	}

	let mut servers = self.candidate_backfill_servers(room_id).await;

	for backfill_server in servers {
		info!("Asking {backfill_server} for event {}", event_id);
		let value = self
			.services
			.sending
			.send_federation_request(
				&backfill_server,
				federation::event::get_event::v1::Request::new(event_id.to_owned()),
			)
			.await
			.and_then(|response| {
				serde_json::from_str::<CanonicalJsonObject>(response.pdu.get()).map_err(|e| {
					err!(BadServerResponse(debug_warn!(
						"Error parsing incoming event {e:?} from {backfill_server}"
					)))
				})
			});
		let pdu = match value {
			| Ok(value) => {
				self.services
					.event_handler
					.handle_incoming_pdu(&backfill_server, room_id, event_id, value, false)
					.boxed()
					.await?;
				debug!("Successfully backfilled {event_id} from {backfill_server}");
				Some(self.get_pdu(event_id).await)
			},
			| Err(e) => {
				warn!("{backfill_server} failed to provide backfill for room {room_id}: {e}");
				None
			},
		};
		if let Some(pdu) = pdu {
			debug!("Fetched {event_id} from {backfill_server}");
			return pdu;
		}
	}

	Err!("No servers could be used to fetch {} in {}.", room_id, event_id)
}

#[implement(super::Service)]
#[tracing::instrument(skip(self, pdu), level = "debug")]
pub async fn backfill_pdu(&self, origin: &ServerName, pdu: Box<RawJsonValue>) -> Result<()> {
	let (room_id, event_id, value) = self.services.event_handler.parse_incoming_pdu(&pdu).await?;

	// Lock so we cannot backfill the same pdu twice at the same time
	let mutex_lock = self
		.services
		.event_handler
		.mutex_federation
		.lock(room_id.as_str())
		.await;

	// Skip the PDU if we already have it as a timeline event
	if let Ok(pdu_id) = self.get_pdu_id(&event_id).await {
		debug!("We already know {event_id} at {pdu_id:?}");
		return Ok(());
	}

	self.services
		.event_handler
		.handle_incoming_pdu(origin, &room_id, &event_id, value, false)
		.boxed()
		.await?;

	let value = self.get_pdu_json(&event_id).await?;

	let pdu = self.get_pdu(&event_id).await?;

	let shortroomid = self.services.short.get_shortroomid(&room_id).await?;

	let insert_lock = self.mutex_insert.lock(room_id.as_str()).await;

	let count: i64 = self.services.globals.next_count().unwrap().try_into()?;

	let pdu_id: RawPduId = PduId {
		shortroomid,
		shorteventid: PduCount::Backfilled(validated!(0 - count)),
	}
	.into();

	// Insert pdu
	self.db.prepend_backfill_pdu(&pdu_id, &event_id, &value);

	drop(insert_lock);

	if pdu.kind == TimelineEventType::RoomMessage {
		let content: ExtractBody = pdu.get_content()?;
		if let Some(body) = content.body {
			self.services.search.index_pdu(shortroomid, &pdu_id, &body);
		}
	}
	drop(mutex_lock);

	debug!("Prepended backfill pdu");
	Ok(())
}

#[implement(super::Service)]
async fn candidate_backfill_servers(&self, room_id: &RoomId) -> HashSet<OwnedServerName> {
	let mut candidate_backfill_servers = HashSet::new();

	let power_levels = self
		.services
		.state_accessor
		.get_room_power_levels(room_id)
		.await;

	// Insert servers of room creators
	if let Some(creators) = &power_levels.rules.privileged_creators {
		for creator in creators {
			candidate_backfill_servers.insert(creator.server_name().to_owned());
		}
	}

	// Insert servers of remote users with higher-than-default PL
	for (user_id, level) in &power_levels.users {
		if !self.services.globals.user_is_local(user_id) && *level > power_levels.users_default {
			candidate_backfill_servers.insert(user_id.server_name().to_owned());
		}
	}

	// Insert the canonical room alias server
	if let Ok(canonical_alias) = self
		.services
		.state_accessor
		.get_canonical_alias(room_id)
		.await
	{
		candidate_backfill_servers.insert(canonical_alias.server_name().to_owned());
	}

	// Insert all trusted servers in the config
	candidate_backfill_servers
		.extend(self.services.server.config.trusted_servers.iter().cloned());

	// Remove our own name, we can't request backfill from ourselves
	candidate_backfill_servers.remove(self.services.globals.server_name());

	// Remove all servers that aren't in the room
	for server in candidate_backfill_servers.clone() {
		if !self
			.services
			.state_cache
			.server_in_room(&server, room_id)
			.await
		{
			candidate_backfill_servers.remove(&server);
		}
	}

	debug!(?candidate_backfill_servers, "Found candidate servers for backfill");
	candidate_backfill_servers
}
