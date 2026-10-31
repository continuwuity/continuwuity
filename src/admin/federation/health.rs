use std::time::Duration;

use clap::Subcommand;
use conduwuit::Result;
use conduwuit_macros::admin_command_dispatch;
use futures::StreamExt;
use ruma::{OwnedRoomOrAliasId, OwnedServerName};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum HealthCommand {
	/// Lists all servers that are currently marked as unhealthy (i.e. we
	/// previously failed to establish direct federation and have not succeeded
	/// since).
	#[clap(name = "list")]
	ListUnhealthyServers {
		/// An optional filter to only check servers that are in a specific room
		/// (ID or alias).
		room: Option<OwnedRoomOrAliasId>,
	},

	/// Displays the current health status of the given remote server.
	#[clap(name = "status")]
	ServerHealthStatus {
		server_name: OwnedServerName,
	},

	/// Forces the server to consider the remote server healthy.
	///
	/// This is generally unnecessary as this already happens when the remote
	/// server makes contact with this server, or if this server successfully
	/// makes a request to the remote server after the backoff window.
	ForceHealthy {
		server_name: OwnedServerName,
	},

	/// Forces the server to consider the remote server unhealthy.
	///
	/// This is generally unnecessary as this already happens when the remote
	/// server fails to resolve or connect, or returns unusual errors.
	/// If you are looking to stop contacting a specific server, take a look at
	/// the `forbidden_remote_server_names` configuration option.
	ForceUnhealthy {
		server_name: OwnedServerName,
		#[arg(long)]
		stale: bool,
	},
}

impl crate::Context<'_> {
	pub(super) async fn list_unhealthy_servers(
		&self,
		room: Option<OwnedRoomOrAliasId>,
	) -> Result {
		let unhealthy = self.services.federation.remote_health();

		if unhealthy.is_empty() {
			return self
				.write_str("No servers are currently tracked as unhealthy.")
				.await;
		}

		let stale_destinations = self.services.federation.stale_destinations();

		let mut candidates: Vec<_> = if let Some(room) = room {
			let room_id = self.services.rooms.alias.resolve(&room).await?;
			self.services
				.rooms
				.state_cache
				.room_servers(&room_id)
				.collect()
				.await
		} else {
			unhealthy
				.keys()
				.chain(stale_destinations.iter())
				.cloned()
				.collect()
		};
		candidates.retain(|name| unhealthy.contains_key(name));
		candidates.sort();
		candidates.dedup();
		if candidates.is_empty() {
			return self
				.write_str("No servers are currently unhealthy meeting the given criteria.")
				.await;
		}
		for server_name in candidates {
			let Some((retries, retry_at)) = unhealthy.get(&server_name) else { continue };
			let is_stale = if stale_destinations.contains(&server_name) {
				"Yes"
			} else {
				"No"
			};
			self.write_str(&format!("| {server_name} | {retries} | {retry_at} | {is_stale} |\n"))
				.await?;
		}

		Ok(())
	}

	pub(super) async fn server_health_status(&self, server_name: OwnedServerName) -> Result {
		let stale = self
			.services
			.federation
			.stale_destinations()
			.contains(&server_name);
		let Some((retries, retry_at_ts)) = self
			.services
			.federation
			.remote_health()
			.get(&server_name)
			.copied()
		else {
			return self
				.write_str(&if stale {
					format!(
						"{server_name} is currently healthy, but may have stale routing \
						 information."
					)
				} else {
					format!("{server_name} is currently healthy.")
				})
				.await;
		};

		let state = if stale {
			"unhealthy with potentially stale routing information"
		} else {
			"unhealthy"
		};
		self.write_str(&format!(
			"{server_name} is currently {state}: {retries} consecutive connections have \
			 failed,and no new connection will be attempted for another {}.",
			conduwuit::utils::time::pretty(Duration::from_millis(retry_at_ts)),
		))
		.await
	}

	pub(super) async fn force_healthy(&self, server_name: OwnedServerName) -> Result {
		self.services.federation.mark_healthy(&server_name);
		self.write_str("Done.").await
	}

	pub(super) async fn force_unhealthy(
		&self,
		server_name: OwnedServerName,
		stale: bool,
	) -> Result {
		if stale {
			self.services
				.federation
				.mark_destination_stale(&server_name);
		}
		self.services.federation.hit_unhealthy(server_name);
		self.write_str("Done.").await
	}
}
