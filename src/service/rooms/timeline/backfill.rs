use std::iter::once;

use conduwuit::RoomVersion;
use conduwuit::{Err, PduEvent};
use conduwuit_core::{
	Result, debug, debug_warn, err, implement, info,
	matrix::{
		event::Event,
		pdu::{PduCount, PduId, RawPduId},
	},
	trace,
	utils::{IterStream, ReadyExt},
	validated, warn,
};
use futures::{FutureExt, StreamExt};
use ruma::{
	CanonicalJsonObject, EventId, Int, RoomId, ServerName,
	api::federation,
	events::{
		StateEventType, TimelineEventType,
		room::{create::RoomCreateEventContent, power_levels::RoomPowerLevelsEventContent},
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

	let power_levels: RoomPowerLevelsEventContent = self
		.services
		.state_accessor
		.room_state_get_content(room_id, &StateEventType::RoomPowerLevels, "")
		.await
		.unwrap_or_default();
	let create_event_content: RoomCreateEventContent = self
		.services
		.state_accessor
		.room_state_get_content(room_id, &StateEventType::RoomCreate, "")
		.await?;
	let create_event = self
		.services
		.state_accessor
		.room_state_get(room_id, &StateEventType::RoomCreate, "")
		.await?;

	let room_version =
		RoomVersion::new(&create_event_content.room_version).expect("supported room version");
	let mut users = power_levels.users.clone();
	if room_version.explicitly_privilege_room_creators {
		users.insert(create_event.sender().to_owned(), Int::MAX);
		if let Some(additional_creators) = &create_event_content.additional_creators {
			for user_id in additional_creators {
				users.insert(user_id.to_owned(), Int::MAX);
			}
		}
	}

	let room_mods = users.iter().filter_map(|(user_id, level)| {
		let remote_powered =
			level > &power_levels.users_default && !self.services.globals.user_is_local(user_id);
		let creator = if room_version.explicitly_privilege_room_creators {
			create_event.sender() == user_id
				|| create_event_content
					.additional_creators
					.as_ref()
					.is_some_and(|c| c.contains(user_id))
		} else {
			false
		};

		if remote_powered || creator {
			debug!(%remote_powered, %creator, "User {user_id} can backfill in room {room_id}");
			Some(user_id.server_name())
		} else {
			debug!(%remote_powered, %creator, "User {user_id} cannot backfill in room {room_id}");
			None
		}
	});

	let canonical_room_alias_server = once(
		self.services
			.state_accessor
			.get_canonical_alias(room_id)
			.await,
	)
	.filter_map(Result::ok)
	.map(|alias| alias.server_name().to_owned())
	.stream();

	let mut servers = room_mods
		.stream()
		.map(ToOwned::to_owned)
		.chain(canonical_room_alias_server)
		.chain(
			self.services
				.server
				.config
				.trusted_servers
				.iter()
				.map(ToOwned::to_owned)
				.stream(),
		)
		.ready_filter(|server_name| !self.services.globals.server_is_ours(server_name))
		.filter_map(|server_name| async move {
			self.services
				.state_cache
				.server_in_room(&server_name, room_id)
				.await
				.then_some(server_name)
		})
		.boxed();

	while let Some(ref backfill_server) = servers.next().await {
		info!("Asking {backfill_server} for backfill in {room_id}");
		let response = self
			.services
			.sending
			.send_federation_request(
				backfill_server,
				federation::backfill::get_backfill::v1::Request {
					room_id: room_id.to_owned(),
					v: vec![first_pdu.1.event_id().to_owned()],
					limit: uint!(100),
				},
			)
			.await;
		match response {
			| Ok(response) => {
				for pdu in response.pdus {
					if let Err(e) = self.backfill_pdu(backfill_server, pdu).boxed().await {
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

	warn!("No servers could backfill, but backfill was needed in room {room_id}");
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

	let power_levels: RoomPowerLevelsEventContent = self
		.services
		.state_accessor
		.room_state_get_content(room_id, &StateEventType::RoomPowerLevels, "")
		.await
		.unwrap_or_default();

	let room_mods = power_levels.users.iter().filter_map(|(user_id, level)| {
		if level > &power_levels.users_default && !self.services.globals.user_is_local(user_id) {
			Some(user_id.server_name())
		} else {
			None
		}
	});

	let canonical_room_alias_server = once(
		self.services
			.state_accessor
			.get_canonical_alias(room_id)
			.await,
	)
	.filter_map(Result::ok)
	.map(|alias| alias.server_name().to_owned())
	.stream();
	let mut servers = room_mods
		.stream()
		.map(ToOwned::to_owned)
		.chain(canonical_room_alias_server)
		.chain(
			self.services
				.server
				.config
				.trusted_servers
				.iter()
				.map(ToOwned::to_owned)
				.stream(),
		)
		.ready_filter(|server_name| !self.services.globals.server_is_ours(server_name))
		.filter_map(|server_name| async move {
			self.services
				.state_cache
				.server_in_room(&server_name, room_id)
				.await
				.then_some(server_name)
		})
		.boxed();

	while let Some(ref backfill_server) = servers.next().await {
		info!("Asking {backfill_server} for event {}", event_id);
		let value = self
			.services
			.sending
			.send_federation_request(backfill_server, federation::event::get_event::v1::Request {
				event_id: event_id.to_owned(),
				include_unredacted_content: Some(false),
			})
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
					.handle_incoming_pdu(backfill_server, room_id, event_id, value, false)
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
		.lock(&room_id)
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

	let insert_lock = self.mutex_insert.lock(&room_id).await;

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
