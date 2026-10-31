use std::fmt::Write;

use conduwuit::{Err, Result, utils::response::LimitReadExt};
use ruma::{MilliSecondsSinceUnixEpoch, OwnedRoomOrAliasId, OwnedServerName};

impl crate::Context<'_> {
	pub(super) async fn disable_room(&self, room: OwnedRoomOrAliasId) -> Result {
		self.bail_restricted()?;
		let room_id = self.services.rooms.alias.resolve(&room).await?;
		self.services.rooms.metadata.disable_room(&room_id, true);
		self.write_str("Room disabled.").await
	}

	pub(super) async fn enable_room(&self, room: OwnedRoomOrAliasId) -> Result {
		self.bail_restricted()?;
		let room_id = self.services.rooms.alias.resolve(&room).await?;
		self.services.rooms.metadata.disable_room(&room_id, false);
		self.write_str("Room enabled.").await
	}

	pub(super) async fn incoming_federation(&self) -> Result {
		let msg = {
			let map = self
				.services
				.rooms
				.event_handler
				.federation_handletime
				.read();

			let mut msg = format!(
				"Handling {} incoming PDUs across {} active transactions:\n",
				map.len(),
				self.services.transactions.txn_active_handle_count()
			);
			for (room_id, (event_id, started_at)) in map.iter() {
				let elapsed = started_at.elapsed();
				writeln!(
					msg,
					"{} {}: {}m{}s",
					room_id,
					event_id,
					elapsed.as_secs() / 60,
					elapsed.as_secs() % 60
				)?;
			}
			msg
		};

		self.write_str(&msg).await
	}

	pub(super) async fn fetch_support(&self, server_name: OwnedServerName) -> Result {
		let response = self
			.services
			.client
			.external_resource
			.get(format!("https://{server_name}/.well-known/matrix/support"))
			.send()
			.await?;

		let text = response
			.limit_read_text(
				self.services
					.config
					.max_request_size
					.try_into()
					.expect("u64 fits into usize"),
			)
			.await?;

		if text.is_empty() {
			return Err!("Response text/body is empty.");
		}

		if text.len() > 1500 {
			return Err!(
				"Response text/body is over 1500 characters, assuming no support well-known.",
			);
		}

		let json: serde_json::Value = match serde_json::from_str(&text) {
			| Ok(json) => json,
			| Err(_) => {
				return Err!("Response text/body is not valid JSON.",);
			},
		};

		let pretty_json: String = match serde_json::to_string_pretty(&json) {
			| Ok(json) => json,
			| Err(_) => {
				return Err!("Response text/body is not valid JSON.",);
			},
		};

		self.write_str(&format!("Got JSON response:\n\n```json\n{pretty_json}\n```"))
			.await
	}

	pub(super) async fn get_signing_keys(&self, server_name: OwnedServerName) -> Result {
		let response = self
			.services
			.server_keys
			.origin_request(server_name, MilliSecondsSinceUnixEpoch::now())
			.await?;
		self.write_str("| Key ID | Public Key | Expired (unix milliseconds) |\n")
			.await?;
		self.write_str("| ------ | ---------- | --------------------------- |\n")
			.await?;
		for (key_id, key) in &response.verify_keys {
			self.write_str(&format!("| {key_id} | {} | |\n", key.key))
				.await?;
		}
		for (key_id, old_key) in &response.old_verify_keys {
			self.write_str(&format!(
				"| {key_id} | {} | {:0<13} |\n",
				old_key.key,
				old_key.expired_ts.get().to_string()
			))
			.await?;
		}

		Ok(())
	}

	pub(super) async fn ping(&self, server: OwnedServerName) -> Result {
		let timer = tokio::time::Instant::now();

		match self
			.services
			.sending
			.send_unauthenticated_request(
				&server,
				ruma::api::federation::discovery::get_server_version::v1::Request::new(),
			)
			.await
		{
			| Err(e) => {
				return Err!("Failed sending federation request to specified server:\n\n{e}");
			},
			| Ok(response) => {
				let ping_time = timer.elapsed();
				let json_text_res = serde_json::to_string_pretty(&response.server);

				let out = if let Ok(json) = json_text_res {
					format!("Got response which took {ping_time:?} time:\n```json\n{json}\n```")
				} else {
					format!("Got non-JSON response which took {ping_time:?} time:\n{response:?}")
				};

				write!(self, "{out}")
			},
		}
		.await
	}
}
