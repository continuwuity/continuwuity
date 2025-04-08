use std::{
	collections::HashMap,
	fmt::Write,
	iter::once,
	time::{Instant, SystemTime},
};

use conduwuit::{
	Error, Result, debug_error, err, info,
	matrix::pdu::{PduEvent, PduId, RawPduId},
	trace, utils,
	utils::{
		stream::{IterStream, ReadyExt},
		string::EMPTY,
	},
	warn,
};
use futures::{FutureExt, StreamExt, TryStreamExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, OwnedRoomId,
	OwnedRoomOrAliasId, OwnedServerName, RoomId, RoomVersionId,
	api::federation::event::get_room_state, events::room::message::RoomMessageEventContent,
};
use service::rooms::{
	short::{ShortEventId, ShortRoomId},
	state_compressor::HashSetCompressStateEvent,
};
use tracing_subscriber::EnvFilter;

use crate::admin_command;

#[admin_command]
pub(super) async fn echo(&self, message: Vec<String>) -> Result<RoomMessageEventContent> {
	let message = message.join(" ");

	Ok(RoomMessageEventContent::notice_plain(message))
}

#[admin_command]
pub(super) async fn get_auth_chain(
	&self,
	event_id: OwnedEventId,
) -> Result<RoomMessageEventContent> {
	let Ok(event) = self.services.rooms.timeline.get_pdu_json(&event_id).await else {
		return Ok(RoomMessageEventContent::notice_plain("Event not found."));
	};

	let room_id_str = event
		.get("room_id")
		.and_then(|val| val.as_str())
		.ok_or_else(|| Error::bad_database("Invalid event in database"))?;

	let room_id = <&RoomId>::try_from(room_id_str)
		.map_err(|_| Error::bad_database("Invalid room id field in event in database"))?;

	let start = Instant::now();
	let count = self
		.services
		.rooms
		.auth_chain
		.event_ids_iter(room_id, once(event_id.as_ref()))
		.ready_filter_map(Result::ok)
		.count()
		.await;

	let elapsed = start.elapsed();
	Ok(RoomMessageEventContent::text_plain(format!(
		"Loaded auth chain with length {count} in {elapsed:?}"
	)))
}

#[admin_command]
pub(super) async fn parse_pdu(&self) -> Result<RoomMessageEventContent> {
	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&EMPTY).trim() != "```"
	{
		return Ok(RoomMessageEventContent::text_plain(
			"Expected code block in command body. Add --help for details.",
		));
	}

	let string = self.body[1..self.body.len().saturating_sub(1)].join("\n");
	match serde_json::from_str(&string) {
		| Ok(value) => match ruma::signatures::reference_hash(&value, &RoomVersionId::V6) {
			| Ok(hash) => {
				let event_id = OwnedEventId::parse(format!("${hash}"));

				match serde_json::from_value::<PduEvent>(
					serde_json::to_value(value).expect("value is json"),
				) {
					| Ok(pdu) => Ok(RoomMessageEventContent::text_plain(format!(
						"EventId: {event_id:?}\n{pdu:#?}"
					))),
					| Err(e) => Ok(RoomMessageEventContent::text_plain(format!(
						"EventId: {event_id:?}\nCould not parse event: {e}"
					))),
				}
			},
			| Err(e) => Ok(RoomMessageEventContent::text_plain(format!(
				"Could not parse PDU JSON: {e:?}"
			))),
		},
		| Err(e) => Ok(RoomMessageEventContent::text_plain(format!(
			"Invalid json in command body: {e}"
		))),
	}
}

#[admin_command]
pub(super) async fn get_pdu(&self, event_id: OwnedEventId) -> Result<RoomMessageEventContent> {
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

	match pdu_json {
		| Ok(json) => {
			let json_text =
				serde_json::to_string_pretty(&json).expect("canonical json is valid json");
			Ok(RoomMessageEventContent::notice_markdown(format!(
				"{}\n```json\n{}\n```",
				if outlier {
					"Outlier (Rejected / Soft Failed) PDU found in our database"
				} else {
					"PDU found in our database"
				},
				json_text
			)))
		},
		| Err(_) => Ok(RoomMessageEventContent::text_plain("PDU not found locally.")),
	}
}

#[admin_command]
pub(super) async fn get_short_pdu(
	&self,
	shortroomid: ShortRoomId,
	shorteventid: ShortEventId,
) -> Result<RoomMessageEventContent> {
	let pdu_id: RawPduId = PduId {
		shortroomid,
		shorteventid: shorteventid.into(),
	}
	.into();

	let pdu_json = self
		.services
		.rooms
		.timeline
		.get_pdu_json_from_id(&pdu_id)
		.await;

	match pdu_json {
		| Ok(json) => {
			let json_text =
				serde_json::to_string_pretty(&json).expect("canonical json is valid json");
			Ok(RoomMessageEventContent::notice_markdown(format!("```json\n{json_text}\n```",)))
		},
		| Err(_) => Ok(RoomMessageEventContent::text_plain("PDU not found locally.")),
	}
}

#[admin_command]
pub(super) async fn get_remote_pdu_list(
	&self,
	server: OwnedServerName,
	force: bool,
) -> Result<RoomMessageEventContent> {
	if !self.services.server.config.allow_federation {
		return Ok(RoomMessageEventContent::text_plain(
			"Federation is disabled on this homeserver.",
		));
	}

	if server == self.services.globals.server_name() {
		return Ok(RoomMessageEventContent::text_plain(
			"Not allowed to send federation requests to ourselves. Please use `get-pdu` for \
			 fetching local PDUs from the database.",
		));
	}

	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&EMPTY).trim() != "```"
	{
		return Ok(RoomMessageEventContent::text_plain(
			"Expected code block in command body. Add --help for details.",
		));
	}

	let list = self
		.body
		.iter()
		.collect::<Vec<_>>()
		.drain(1..self.body.len().saturating_sub(1))
		.filter_map(|pdu| EventId::parse(pdu).ok())
		.collect::<Vec<_>>();

	let mut failed_count: usize = 0;
	let mut success_count: usize = 0;

	for pdu in list {
		if force {
			match self.get_remote_pdu(Box::from(pdu), server.clone()).await {
				| Err(e) => {
					failed_count = failed_count.saturating_add(1);
					self.services
						.admin
						.send_message(RoomMessageEventContent::text_plain(format!(
							"Failed to get remote PDU, ignoring error: {e}"
						)))
						.await
						.ok();
					warn!("Failed to get remote PDU, ignoring error: {e}");
				},
				| _ => {
					success_count = success_count.saturating_add(1);
				},
			}
		} else {
			self.get_remote_pdu(Box::from(pdu), server.clone()).await?;
			success_count = success_count.saturating_add(1);
		}
	}

	Ok(RoomMessageEventContent::text_plain(format!(
		"Fetched {success_count} remote PDUs successfully with {failed_count} failures"
	)))
}

#[admin_command]
pub(super) async fn get_remote_pdu(
	&self,
	event_id: Box<EventId>,
	server: Box<ServerName>,
) -> Result<RoomMessageEventContent> {
	if !self.services.server.config.allow_federation {
		return Ok(RoomMessageEventContent::text_plain(
			"Federation is disabled on this homeserver.",
		));
	}

	if server == self.services.globals.server_name() {
		return Ok(RoomMessageEventContent::text_plain(
			"Not allowed to send federation requests to ourselves. Please use `get-pdu` for \
			 fetching local PDUs.",
		));
	}

	match self
		.services
		.sending
		.send_federation_request(&server, ruma::api::federation::event::get_event::v1::Request {
			event_id: event_id.clone().into(),
			include_unredacted_content: None,
		})
		.await
	{
		| Ok(response) => {
			let json: CanonicalJsonObject =
				serde_json::from_str(response.pdu.get()).map_err(|e| {
					warn!(
						"Requested event ID {event_id} from server but failed to convert from \
						 RawValue to CanonicalJsonObject (malformed event/response?): {e}"
					);
					Error::BadRequest(
						ErrorKind::Unknown,
						"Received response from server but failed to parse PDU",
					)
				})?;

			trace!("Attempting to parse PDU: {:?}", &response.pdu);
			let _parsed_pdu = {
				let parsed_result = self
					.services
					.rooms
					.event_handler
					.parse_incoming_pdu(&response.pdu)
					.await;

				let (event_id, value, room_id) = match parsed_result {
					| Ok(t) => t,
					| Err(e) => {
						warn!("Failed to parse PDU: {e}");
						info!("Full PDU: {:?}", &response.pdu);
						return Ok(RoomMessageEventContent::text_plain(format!(
							"Failed to parse PDU remote server {server} sent us: {e}"
						)));
					},
				};

				vec![(event_id, value, room_id)]
			};

			info!("Attempting to handle event ID {event_id} as backfilled PDU");
			self.services
				.rooms
				.timeline
				.backfill_pdu(&server, response.pdu)
				.boxed()
				.await?;

			let json_text =
				serde_json::to_string_pretty(&json).expect("canonical json is valid json");

			Ok(RoomMessageEventContent::notice_markdown(format!(
				"{}\n```json\n{}\n```",
				"Got PDU from specified server and handled as backfilled PDU successfully. \
				 Event body:",
				json_text
			)))
		},
		| Err(e) => Ok(RoomMessageEventContent::text_plain(format!(
			"Remote server did not have PDU or failed sending request to remote server: {e}"
		))),
	}
}

#[admin_command]
pub(super) async fn get_room_state(
	&self,
	room: OwnedRoomOrAliasId,
) -> Result<RoomMessageEventContent> {
	let room_id = self.services.rooms.alias.resolve(&room).await?;
	let room_state: Vec<_> = self
		.services
		.rooms
		.state_accessor
		.room_state_full_pdus(&room_id)
		.map_ok(PduEvent::into_state_event)
		.try_collect()
		.await?;

	if room_state.is_empty() {
		return Ok(RoomMessageEventContent::text_plain(
			"Unable to find room state in our database (vector is empty)",
		));
	}

	let json = serde_json::to_string_pretty(&room_state).map_err(|e| {
		warn!("Failed converting room state vector in our database to pretty JSON: {e}");
		Error::bad_database(
			"Failed to convert room state events to pretty JSON, possible invalid room state \
			 events in our database",
		)
	})?;

	Ok(RoomMessageEventContent::notice_markdown(format!("```json\n{json}\n```")))
}

#[admin_command]
pub(super) async fn ping(&self, server: OwnedServerName) -> Result<RoomMessageEventContent> {
	if server == self.services.globals.server_name() {
		return Ok(RoomMessageEventContent::text_plain(
			"Not allowed to send federation requests to ourselves.",
		));
	}

	let timer = tokio::time::Instant::now();

	match self
		.services
		.sending
		.send_federation_request(
			&server,
			ruma::api::federation::discovery::get_server_version::v1::Request {},
		)
		.await
	{
		| Ok(response) => {
			let ping_time = timer.elapsed();

			let json_text_res = serde_json::to_string_pretty(&response.server);

			if let Ok(json) = json_text_res {
				return Ok(RoomMessageEventContent::notice_markdown(format!(
					"Got response which took {ping_time:?} time:\n```json\n{json}\n```"
				)));
			}

			Ok(RoomMessageEventContent::text_plain(format!(
				"Got non-JSON response which took {ping_time:?} time:\n{response:?}"
			)))
		},
		| Err(e) => {
			warn!(
				"Failed sending federation request to specified server from ping debug command: \
				 {e}"
			);
			Ok(RoomMessageEventContent::text_plain(format!(
				"Failed sending federation request to specified server:\n\n{e}",
			)))
		},
	}
}

#[admin_command]
pub(super) async fn force_device_list_updates(&self) -> Result<RoomMessageEventContent> {
	// Force E2EE device list updates for all users
	self.services
		.users
		.stream()
		.for_each(|user_id| self.services.users.mark_device_key_update(user_id))
		.await;

	Ok(RoomMessageEventContent::text_plain(
		"Marked all devices for all users as having new keys to update",
	))
}

#[admin_command]
pub(super) async fn change_log_level(
	&self,
	filter: Option<String>,
	reset: bool,
) -> Result<RoomMessageEventContent> {
	let handles = &["console"];

	if reset {
		let old_filter_layer = match EnvFilter::try_new(&self.services.server.config.log) {
			| Ok(s) => s,
			| Err(e) => {
				return Ok(RoomMessageEventContent::text_plain(format!(
					"Log level from config appears to be invalid now: {e}"
				)));
			},
		};

		match self
			.services
			.server
			.log
			.reload
			.reload(&old_filter_layer, Some(handles))
		{
			| Ok(()) => {
				return Ok(RoomMessageEventContent::text_plain(format!(
					"Successfully changed log level back to config value {}",
					self.services.server.config.log
				)));
			},
			| Err(e) => {
				return Ok(RoomMessageEventContent::text_plain(format!(
					"Failed to modify and reload the global tracing log level: {e}"
				)));
			},
		}
	}

	if let Some(filter) = filter {
		let new_filter_layer = match EnvFilter::try_new(filter) {
			| Ok(s) => s,
			| Err(e) => {
				return Ok(RoomMessageEventContent::text_plain(format!(
					"Invalid log level filter specified: {e}"
				)));
			},
		};

		match self
			.services
			.server
			.log
			.reload
			.reload(&new_filter_layer, Some(handles))
		{
			| Ok(()) => {
				return Ok(RoomMessageEventContent::text_plain("Successfully changed log level"));
			},
			| Err(e) => {
				return Ok(RoomMessageEventContent::text_plain(format!(
					"Failed to modify and reload the global tracing log level: {e}"
				)));
			},
		}
	}

	Ok(RoomMessageEventContent::text_plain("No log level was specified."))
}

#[admin_command]
pub(super) async fn sign_json(&self) -> Result<RoomMessageEventContent> {
	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&"").trim() != "```"
	{
		return Ok(RoomMessageEventContent::text_plain(
			"Expected code block in command body. Add --help for details.",
		));
	}

	let string = self.body[1..self.body.len().checked_sub(1).unwrap()].join("\n");
	match serde_json::from_str(&string) {
		| Ok(mut value) => {
			self.services
				.server_keys
				.sign_json(&mut value)
				.expect("our request json is what ruma expects");
			let json_text =
				serde_json::to_string_pretty(&value).expect("canonical json is valid json");
			Ok(RoomMessageEventContent::text_plain(json_text))
		},
		| Err(e) => Ok(RoomMessageEventContent::text_plain(format!("Invalid json: {e}"))),
	}
}

#[admin_command]
pub(super) async fn verify_json(&self) -> Result<RoomMessageEventContent> {
	if self.body.len() < 2
		|| !self.body[0].trim().starts_with("```")
		|| self.body.last().unwrap_or(&"").trim() != "```"
	{
		return Ok(RoomMessageEventContent::text_plain(
			"Expected code block in command body. Add --help for details.",
		));
	}

	let string = self.body[1..self.body.len().checked_sub(1).unwrap()].join("\n");
	match serde_json::from_str::<CanonicalJsonObject>(&string) {
		| Ok(value) => match self.services.server_keys.verify_json(&value, None).await {
			| Ok(()) => Ok(RoomMessageEventContent::text_plain("Signature correct")),
			| Err(e) => Ok(RoomMessageEventContent::text_plain(format!(
				"Signature verification failed: {e}"
			))),
		},
		| Err(e) => Ok(RoomMessageEventContent::text_plain(format!("Invalid json: {e}"))),
	}
}

#[admin_command]
pub(super) async fn verify_pdu(&self, event_id: OwnedEventId) -> Result<RoomMessageEventContent> {
	let mut event = self.services.rooms.timeline.get_pdu_json(&event_id).await?;

	event.remove("event_id");
	let msg = match self.services.server_keys.verify_event(&event, None).await {
		| Ok(ruma::signatures::Verified::Signatures) =>
			"signatures OK, but content hash failed (redaction).",
		| Ok(ruma::signatures::Verified::All) => "signatures and hashes OK.",
		| Err(e) => return Err(e),
	};

	Ok(RoomMessageEventContent::notice_plain(msg))
}

#[admin_command]
#[tracing::instrument(skip(self))]
pub(super) async fn first_pdu_in_room(
	&self,
	room_id: OwnedRoomId,
) -> Result<RoomMessageEventContent> {
	if !self
		.services
		.rooms
		.state_cache
		.server_in_room(&self.services.server.name, &room_id)
		.await
	{
		return Ok(RoomMessageEventContent::text_plain(
			"We are not participating in the room / we don't know about the room ID.",
		));
	}

	let first_pdu = self
		.services
		.rooms
		.timeline
		.first_pdu_in_room(&room_id)
		.await
		.map_err(|_| Error::bad_database("Failed to find the first PDU in database"))?;

	Ok(RoomMessageEventContent::text_plain(format!("{first_pdu:?}")))
}

#[admin_command]
#[tracing::instrument(skip(self))]
pub(super) async fn latest_pdu_in_room(
	&self,
	room_id: OwnedRoomId,
) -> Result<RoomMessageEventContent> {
	if !self
		.services
		.rooms
		.state_cache
		.server_in_room(&self.services.server.name, &room_id)
		.await
	{
		return Ok(RoomMessageEventContent::text_plain(
			"We are not participating in the room / we don't know about the room ID.",
		));
	}

	let latest_pdu = self
		.services
		.rooms
		.timeline
		.latest_pdu_in_room(&room_id)
		.await
		.map_err(|_| Error::bad_database("Failed to find the latest PDU in database"))?;

	Ok(RoomMessageEventContent::text_plain(format!("{latest_pdu:?}")))
}

#[admin_command]
#[tracing::instrument(skip(self))]
pub(super) async fn force_set_room_state_from_server(
	&self,
	room_id: OwnedRoomId,
	server_name: OwnedServerName,
) -> Result<RoomMessageEventContent> {
	if !self
		.services
		.rooms
		.state_cache
		.server_in_room(&self.services.server.name, &room_id)
		.await
	{
		return Ok(RoomMessageEventContent::text_plain(
			"We are not participating in the room / we don't know about the room ID.",
		));
	}

	let first_pdu = self
		.services
		.rooms
		.timeline
		.latest_pdu_in_room(&room_id)
		.await
		.map_err(|_| Error::bad_database("Failed to find the latest PDU in database"))?;

	let room_version = self.services.rooms.state.get_room_version(&room_id).await?;

	let mut state: HashMap<u64, OwnedEventId> = HashMap::new();

	let remote_state_response = self
		.services
		.sending
		.send_federation_request(&server_name, get_room_state::v1::Request {
			room_id: room_id.clone().into(),
			event_id: first_pdu.event_id.clone(),
		})
		.boxed()
		.await?;

	for pdu in remote_state_response.pdus.clone() {
		match self
			.services
			.rooms
			.event_handler
			.parse_incoming_pdu(&pdu)
			.boxed()
			.await
		{
			| Ok(t) => t,
			| Err(e) => {
				warn!("Could not parse PDU, ignoring: {e}");
				continue;
			},
		};
	}

	info!("Going through room_state response PDUs");
	for result in remote_state_response.pdus.iter().map(|pdu| {
		self.services
			.server_keys
			.validate_and_add_event_id(pdu, &room_version)
	}) {
		let Ok((event_id, value)) = result.await else {
			continue;
		};

		let pdu = PduEvent::from_id_val(&event_id, value.clone()).map_err(|e| {
			debug_error!("Invalid PDU in fetching remote room state PDUs response: {value:#?}");
			err!(BadServerResponse(debug_error!("Invalid PDU in send_join response: {e:?}")))
		})?;

		self.services
			.rooms
			.outlier
			.add_pdu_outlier(&event_id, &value);

		if let Some(state_key) = &pdu.state_key {
			let shortstatekey = self
				.services
				.rooms
				.short
				.get_or_create_shortstatekey(&pdu.kind.to_string().into(), state_key)
				.await;

			state.insert(shortstatekey, pdu.event_id.clone());
		}
	}

	info!("Going through auth_chain response");
	for result in remote_state_response.auth_chain.iter().map(|pdu| {
		self.services
			.server_keys
			.validate_and_add_event_id(pdu, &room_version)
	}) {
		let Ok((event_id, value)) = result.await else {
			continue;
		};

		self.services
			.rooms
			.outlier
			.add_pdu_outlier(&event_id, &value);
	}

	let new_room_state = self
		.services
		.rooms
		.event_handler
		.resolve_state(&room_id, &room_version, state)
		.boxed()
		.await?;

	info!("Forcing new room state");
	let HashSetCompressStateEvent {
		shortstatehash: short_state_hash,
		added,
		removed,
	} = self
		.services
		.rooms
		.state_compressor
		.save_state(room_id.clone().as_ref(), new_room_state)
		.await?;

	let state_lock = self.services.rooms.state.mutex.lock(&*room_id).await;
	self.services
		.rooms
		.state
		.force_state(room_id.clone().as_ref(), short_state_hash, added, removed, &state_lock)
		.await?;

	info!(
		"Updating joined counts for room just in case (e.g. we may have found a difference in \
		 the room's m.room.member state"
	);
	self.services
		.rooms
		.state_cache
		.update_joined_count(&room_id)
		.await;

	drop(state_lock);

	Ok(RoomMessageEventContent::text_plain(
		"Successfully forced the room state from the requested remote server.",
	))
}

#[admin_command]
pub(super) async fn get_signing_keys(
	&self,
	server_name: Option<OwnedServerName>,
	notary: Option<OwnedServerName>,
	query: bool,
) -> Result<RoomMessageEventContent> {
	let server_name = server_name.unwrap_or_else(|| self.services.server.name.clone().into());

	if let Some(notary) = notary {
		let signing_keys = self
			.services
			.server_keys
			.notary_request(&notary, &server_name)
			.await?;

		return Ok(RoomMessageEventContent::notice_markdown(format!(
			"```rs\n{signing_keys:#?}\n```"
		)));
	}

	let signing_keys = if query {
		self.services
			.server_keys
			.server_request(&server_name)
			.await?
	} else {
		self.services
			.server_keys
			.signing_keys_for(&server_name)
			.await?
	};

	Ok(RoomMessageEventContent::notice_markdown(format!(
		"```rs\n{signing_keys:#?}\n```"
	)))
}

#[admin_command]
pub(super) async fn get_verify_keys(
	&self,
	server_name: Option<OwnedServerName>,
) -> Result<RoomMessageEventContent> {
	let server_name = server_name.unwrap_or_else(|| self.services.server.name.clone().into());

	let keys = self
		.services
		.server_keys
		.verify_keys_for(&server_name)
		.await;

	let mut out = String::new();
	writeln!(out, "| Key ID | Public Key |")?;
	writeln!(out, "| --- | --- |")?;
	for (key_id, key) in keys {
		writeln!(out, "| {key_id} | {key:?} |")?;
	}

	Ok(RoomMessageEventContent::notice_markdown(out))
}

#[admin_command]
pub(super) async fn resolve_true_destination(
	&self,
	server_name: OwnedServerName,
	no_cache: bool,
) -> Result<RoomMessageEventContent> {
	if !self.services.server.config.allow_federation {
		return Ok(RoomMessageEventContent::text_plain(
			"Federation is disabled on this homeserver.",
		));
	}

	if server_name == self.services.server.name {
		return Ok(RoomMessageEventContent::text_plain(
			"Not allowed to send federation requests to ourselves. Please use `get-pdu` for \
			 fetching local PDUs.",
		));
	}

	let actual = self
		.services
		.resolver
		.resolve_actual_dest(&server_name, !no_cache)
		.await?;

	let msg = format!("Destination: {}\nHostname URI: {}", actual.dest, actual.host,);

	Ok(RoomMessageEventContent::text_markdown(msg))
}

#[admin_command]
pub(super) async fn memory_stats(&self, opts: Option<String>) -> Result<RoomMessageEventContent> {
	const OPTS: &str = "abcdefghijklmnopqrstuvwxyz";

	let opts: String = OPTS
		.chars()
		.filter(|&c| {
			let allow_any = opts.as_ref().is_some_and(|opts| opts == "*");

			let allow = allow_any || opts.as_ref().is_some_and(|opts| opts.contains(c));

			!allow
		})
		.collect();

	let stats = conduwuit::alloc::memory_stats(&opts).unwrap_or_default();

	self.write_str("```\n").await?;
	self.write_str(&stats).await?;
	self.write_str("\n```").await?;

	Ok(RoomMessageEventContent::text_plain(""))
}

#[cfg(tokio_unstable)]
#[admin_command]
pub(super) async fn runtime_metrics(&self) -> Result<RoomMessageEventContent> {
	let out = self.services.server.metrics.runtime_metrics().map_or_else(
		|| "Runtime metrics are not available.".to_owned(),
		|metrics| {
			format!(
				"```rs\nnum_workers: {}\nnum_alive_tasks: {}\nglobal_queue_depth: {}\n```",
				metrics.num_workers(),
				metrics.num_alive_tasks(),
				metrics.global_queue_depth()
			)
		},
	);

	Ok(RoomMessageEventContent::text_markdown(out))
}

#[cfg(not(tokio_unstable))]
#[admin_command]
pub(super) async fn runtime_metrics(&self) -> Result<RoomMessageEventContent> {
	Ok(RoomMessageEventContent::text_markdown(
		"Runtime metrics require building with `tokio_unstable`.",
	))
}

#[cfg(tokio_unstable)]
#[admin_command]
pub(super) async fn runtime_interval(&self) -> Result<RoomMessageEventContent> {
	let out = self.services.server.metrics.runtime_interval().map_or_else(
		|| "Runtime metrics are not available.".to_owned(),
		|metrics| format!("```rs\n{metrics:#?}\n```"),
	);

	Ok(RoomMessageEventContent::text_markdown(out))
}

#[cfg(not(tokio_unstable))]
#[admin_command]
pub(super) async fn runtime_interval(&self) -> Result<RoomMessageEventContent> {
	Ok(RoomMessageEventContent::text_markdown(
		"Runtime metrics require building with `tokio_unstable`.",
	))
}

#[admin_command]
pub(super) async fn time(&self) -> Result<RoomMessageEventContent> {
	let now = SystemTime::now();
	Ok(RoomMessageEventContent::text_markdown(utils::time::format(now, "%+")))
}

#[admin_command]
pub(super) async fn list_dependencies(&self, names: bool) -> Result<RoomMessageEventContent> {
	if names {
		let out = info::cargo::dependencies_names().join(" ");
		return Ok(RoomMessageEventContent::notice_markdown(out));
	}

	let deps = info::cargo::dependencies();
	let mut out = String::new();
	writeln!(out, "| name | version | features |")?;
	writeln!(out, "| ---- | ------- | -------- |")?;
	for (name, dep) in deps {
		let version = dep.try_req().unwrap_or("*");
		let feats = dep.req_features();
		let feats = if !feats.is_empty() {
			feats.join(" ")
		} else {
			String::new()
		};
		writeln!(out, "| {name} | {version} | {feats} |")?;
	}

	Ok(RoomMessageEventContent::notice_markdown(out))
}

#[admin_command]
pub(super) async fn database_stats(
	&self,
	property: Option<String>,
	map: Option<String>,
) -> Result<RoomMessageEventContent> {
	let map_name = map.as_ref().map_or(EMPTY, String::as_str);
	let property = property.unwrap_or_else(|| "rocksdb.stats".to_owned());
	self.services
		.db
		.iter()
		.filter(|&(&name, _)| map_name.is_empty() || map_name == name)
		.try_stream()
		.try_for_each(|(&name, map)| {
			let res = map.property(&property).expect("invalid property");
			writeln!(self, "##### {name}:\n```\n{}\n```", res.trim())
		})
		.await?;

	Ok(RoomMessageEventContent::notice_plain(""))
}

#[admin_command]
pub(super) async fn database_files(
	&self,
	map: Option<String>,
	level: Option<i32>,
) -> Result<RoomMessageEventContent> {
	let mut files: Vec<_> = self.services.db.db.file_list().collect::<Result<_>>()?;

	files.sort_by_key(|f| f.name.clone());

	writeln!(self, "| lev  | sst  | keys | dels | size | column |").await?;
	writeln!(self, "| ---: | :--- | ---: | ---: | ---: | :---   |").await?;
	files
		.into_iter()
		.filter(|file| {
			map.as_deref()
				.is_none_or(|map| map == file.column_family_name)
		})
		.filter(|file| level.as_ref().is_none_or(|&level| level == file.level))
		.try_stream()
		.try_for_each(|file| {
			writeln!(
				self,
				"| {} | {:<13} | {:7}+ | {:4}- | {:9} | {} |",
				file.level,
				file.name,
				file.num_entries,
				file.num_deletions,
				file.size,
				file.column_family_name,
			)
		})
		.await?;

	Ok(RoomMessageEventContent::notice_plain(""))
}

#[admin_command]
pub(super) async fn trim_memory(&self) -> Result<RoomMessageEventContent> {
	conduwuit::alloc::trim(None)?;

	writeln!(self, "done").await?;

	Ok(RoomMessageEventContent::notice_plain(""))
}
