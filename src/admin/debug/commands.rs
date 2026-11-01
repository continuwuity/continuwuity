use std::{collections::HashMap, time::SystemTime};

use conduwuit::{
	Err, Result,
	matrix::pdu::PduEvent,
	utils,
	utils::{
		stream::{IterStream, ReadyExt},
		string::EMPTY,
	},
};
use futures::{StreamExt, TryStreamExt};
use lettre::message::Mailbox;
use resolvematrix::{
	resolution::ResolvedDestination,
	server::{MatrixResolver, MatrixResolverBuilder},
};
use ruma::{
	CanonicalJsonObject, EventId, OwnedEventId, OwnedRoomId, OwnedServerName, RoomVersionId,
};
use tracing_subscriber::EnvFilter;

use crate::PAGE_SIZE;

impl crate::Context<'_> {
	pub(super) async fn echo(&self, message: Vec<String>) -> Result {
		let message = message.join(" ");
		self.write_str(&message).await
	}

	pub(super) async fn parse_pdu(&self) -> Result {
		if self.body.len() < 2
			|| !self.body[0].trim().starts_with("```")
			|| self.body.last().unwrap_or(&EMPTY).trim() != "```"
		{
			return Err!("Expected code block in command body. Add --help for details.");
		}

		let string = self.body[1..self.body.len().saturating_sub(1)].join("\n");
		let room_version_rules = RoomVersionId::V12.rules().unwrap();

		match serde_json::from_str(&string) {
			| Err(e) => return Err!("Invalid json in command body: {e}"),
			| Ok(value) => match ruma::signatures::reference_hash(&value, &room_version_rules) {
				| Err(e) => return Err!("Could not parse PDU JSON: {e:?}"),
				| Ok(hash) => {
					let event_id = EventId::parse(format!("${hash}"));
					match serde_json::from_value::<PduEvent>(serde_json::to_value(value)?) {
						| Err(e) =>
							return Err!("EventId: {event_id:?}\nCould not parse event: {e}"),
						| Ok(pdu) => write!(self, "EventId: {event_id:?}\n{pdu:#?}"),
					}
				},
			},
		}
		.await
	}

	pub(super) async fn force_device_list_updates(&self) -> Result {
		// Force E2EE device list updates for all users
		self.services
			.users
			.stream_local_users()
			.for_each(async |user_id| self.services.users.mark_device_key_update(&user_id).await)
			.await;

		write!(self, "Marked all devices for all users as having new keys to update").await
	}

	pub(super) async fn change_log_level(&self, filter: Option<String>, reset: bool) -> Result {
		let handles = &["console"];

		if reset {
			let old_filter_layer = match EnvFilter::try_new(&self.services.server.config.log) {
				| Ok(s) => s,
				| Err(e) => return Err!("Log level from config appears to be invalid now: {e}"),
			};

			match self
				.services
				.server
				.log
				.reload
				.reload(&old_filter_layer, Some(handles))
			{
				| Err(e) => {
					return Err!("Failed to modify and reload the global tracing log level: {e}");
				},
				| Ok(()) => {
					let value = &self.services.server.config.log;
					let out =
						format!("Successfully changed log level back to config value {value}");
					return self.write_str(&out).await;
				},
			}
		}

		if let Some(filter) = filter {
			let new_filter_layer = match EnvFilter::try_new(filter) {
				| Ok(s) => s,
				| Err(e) => return Err!("Invalid log level filter specified: {e}"),
			};

			match self
				.services
				.server
				.log
				.reload
				.reload(&new_filter_layer, Some(handles))
			{
				| Ok(()) => {
					return self.write_str("Successfully changed log level").await;
				},
				| Err(e) => {
					return Err!("Failed to modify and reload the global tracing log level: {e}");
				},
			}
		}

		Err!("No log level was specified.")
	}

	pub(super) async fn verify_json(&self) -> Result {
		if self.body.len() < 2
			|| !self.body[0].trim().starts_with("```")
			|| self.body.last().unwrap_or(&"").trim() != "```"
		{
			return Err!("Expected code block in command body. Add --help for details.");
		}

		let string = self.body[1..self.body.len().checked_sub(1).unwrap()].join("\n");
		let room_version_rules = RoomVersionId::V12.rules().unwrap();

		match serde_json::from_str::<CanonicalJsonObject>(&string) {
			| Err(e) => return Err!("Invalid json: {e}"),
			| Ok(value) => match self
				.services
				.server_keys
				.verify_event_json(&value, &room_version_rules)
				.await
			{
				| Err(e) => return Err!("Signature verification failed: {e}"),
				| Ok(_) => write!(self, "Signature correct"),
			},
		}
		.await
	}

	pub(super) async fn verify_pdu(&self, event_id: OwnedEventId) -> Result {
		use ruma::signatures::Verified;

		let mut event = self.services.rooms.timeline.get_pdu_json(&event_id).await?;
		let room_version_rules = RoomVersionId::V12.rules().unwrap();

		event.remove("event_id");
		let msg = match self
			.services
			.server_keys
			.verify_event_json(&event, &room_version_rules)
			.await
		{
			| Err(e) => return Err(e),
			| Ok(Verified::Signatures) => "signatures OK, but content hash failed (redaction).",
			| Ok(Verified::All) => "signatures and hashes OK.",
		};

		self.write_str(msg).await
	}

	pub(super) async fn resolve_true_destination(
		&self,
		server_name: OwnedServerName,
		no_cache: bool,
	) -> Result {
		if !self.services.server.config.allow_federation {
			return Err!("Federation is disabled on this homeserver.",);
		}

		if server_name == self.services.server.name {
			return Err!(
				"Not allowed to send federation requests to ourselves. Please use `get-pdu` for \
				 fetching local PDUs.",
			);
		}

		let resolver: &MatrixResolver = if no_cache {
			&MatrixResolverBuilder::new()
				.dangerous_tls_accept_invalid_certs(self.services.server.config.allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure)
				.http_client(self.services.client.dns.clone())
				.build()?
		} else {
			&self.services.client.matrix_resolver
		};

		let actual = resolver.resolve_server(server_name.as_str()).await?;

		let destination = match actual.destination {
			| ResolvedDestination::Literal(addr) => addr.to_string(),
			| ResolvedDestination::Named(host, port) => format!("{host}:{port}"),
		};

		let msg = format!(
			"Destination: {}\nHostname URI (SNI): {}\nIs override?: {}\nResolution step: {}",
			destination, actual.host, actual.is_override, actual.resolution_step
		);
		self.write_str(&msg).await
	}

	pub(super) async fn memory_stats(&self, opts: Option<String>) -> Result {
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
		Ok(())
	}

	#[cfg(tokio_unstable)]

	pub(super) async fn runtime_metrics(&self) -> Result {
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

		self.write_str(&out).await
	}

	#[cfg(not(tokio_unstable))]
	pub(super) async fn runtime_metrics(&self) -> Result {
		self.write_str("Runtime metrics require building with `tokio_unstable`.")
			.await
	}

	#[cfg(tokio_unstable)]

	pub(super) async fn runtime_interval(&self) -> Result {
		let out = self.services.server.metrics.runtime_interval().map_or_else(
			|| "Runtime metrics are not available.".to_owned(),
			|metrics| format!("```rs\n{metrics:#?}\n```"),
		);

		self.write_str(&out).await
	}

	#[cfg(not(tokio_unstable))]
	pub(super) async fn runtime_interval(&self) -> Result {
		self.write_str("Runtime metrics require building with `tokio_unstable`.")
			.await
	}

	pub(super) async fn time(&self) -> Result {
		let now = SystemTime::now();
		let now = utils::time::format(now, "%+");

		self.write_str(&now).await
	}

	pub(super) async fn database_stats(
		&self,
		property: Option<String>,
		map: Option<String>,
	) -> Result {
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
			.await
	}

	pub(super) async fn database_files(&self, map: Option<String>, level: Option<i32>) -> Result {
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
			.await
	}

	pub(super) async fn trim_memory(&self) -> Result {
		conduwuit::alloc::trim(None)?;

		writeln!(self, "done").await
	}

	pub(super) async fn send_test_email(&self) -> Result {
		self.bail_restricted()?;

		let mailer = self.services.mailer.expect_mailer()?;
		let Some(sender) = self.sender else {
			return Err!("No sender user provided in context");
		};

		let Some(email) = self
			.services
			.threepid
			.get_email_for_localpart(sender.localpart())
			.await
		else {
			return Err!("{} has no associated email address", sender);
		};

		mailer
			.send(Mailbox::new(None, email.clone()), service::mailer::messages::Test)
			.await?;

		self.write_str(&format!("Test email successfully sent to {email}"))
			.await?;

		Ok(())
	}

	pub(super) async fn rooms_by_extremity_count(&self, page: Option<usize>) -> Result {
		let page = page.unwrap_or(1);
		// My Giant Chain:tm:
		let mapped: HashMap<OwnedRoomId, u64> = self
			.services
			.rooms
			.state
			.all_forward_extremities()
			.ready_fold(HashMap::new(), move |mut map, (room_id, _)| {
				let count: u64 = map.get(&room_id).copied().unwrap_or(0);
				map.insert(room_id, count.saturating_add(1));
				map
			})
			.await
			.into_iter()
			.filter_map(|(room_id, count)| (count >= 2).then_some((room_id, count)))
			.collect();
		if mapped.is_empty() {
			return Err!("No more rooms.");
		}

		let mut rooms = mapped.keys().collect::<Vec<_>>();
		rooms.sort_by_key(|room_id| {
			mapped
				.get(*room_id)
				.copied()
				.expect("keys must have values")
		});
		rooms.reverse();

		let body = rooms
			.into_iter()
			.stream()
			.skip(page.saturating_sub(1).saturating_mul(PAGE_SIZE))
			.take(PAGE_SIZE)
			.map(|room_id| {
				format!(
					"{room_id}: {}",
					mapped.get(room_id).copied().expect("keys must have values")
				)
			})
			.collect::<Vec<_>>()
			.await;

		self.write_str(&format!(
			"Rooms by extremity count ({}):\n```\n{}\n```",
			body.len(),
			body.join("\n")
		))
		.await
	}
}
