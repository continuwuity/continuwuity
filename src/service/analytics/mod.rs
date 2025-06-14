//! # Analytics service
//!
//! This service is responsible for collecting and uploading anonymous server
//! metadata to help improve continuwuity development.
//!
//! All requests are signed with the server's federation signing key for
//! authentication. This service respects the `allow_analytics` configuration
//! option and is enabled by default.
//!
//! Analytics are sent on startup (with up to 5 minutes jitter) and every 12
//! hours thereafter (with up to 30 minutes jitter) to distribute load.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use conduwuit::{
	Result, Server, debug, err, info,
	version::{self, user_agent},
	warn,
};
use database::{Deserialized, Map};
use rand::Rng;
use ruma::ServerName;
use serde::{Deserialize, Serialize};
use tokio::{
	sync::Notify,
	time::{MissedTickBehavior, interval},
};

use crate::{Dep, client, config, federation, globals, server_keys, users};

extern crate conduwuit_build_metadata as build_metadata;

pub struct Service {
	interval: Duration,
	jitter: Duration,
	startup_jitter: Duration,
	interrupt: Notify,
	db: Arc<Map>,
	services: Services,
}

struct Services {
	client: Dep<client::Service>,
	globals: Dep<globals::Service>,
	server_keys: Dep<server_keys::Service>,
	federation: Dep<federation::Service>,
	users: Dep<users::Service>,
	server: Arc<Server>,
	config: Dep<config::Service>,
}

#[derive(Debug, Serialize)]
struct AnalyticsPayload {
	server_name: String,
	version: &'static str,
	commit_hash: Option<&'static str>,
	user_count: usize,
	federation_enabled: bool,
	room_creation_allowed: bool,
	public_room_directory_over_federation: bool,
	build_profile: &'static str,
	opt_level: &'static str,
	rustc_version: &'static str,
	features: Vec<&'static str>,
	host: &'static str,
	target: &'static str,
	// the following can all be derived from the target
	target_arch: &'static str,
	target_os: &'static str,
	target_env: &'static str,
	target_family: &'static str,
}

#[derive(Debug, Deserialize)]
struct AnalyticsResponse {
	success: bool,
	message: Option<String>,
}

const ANALYTICS_URL: &str = "https://analytics.continuwuity.org/api/v1/metrics";
const ANALYTICS_SERVERNAME: &str = "analytics.continuwuity.org";
const ANALYTICS_INTERVAL: u64 = 43200; // 12 hours in seconds
const ANALYTICS_JITTER: u64 = 1800; // 30 minutes in seconds
const ANALYTICS_STARTUP_JITTER: u64 = 300; // 5 minutes in seconds
const LAST_ANALYTICS_TIMESTAMP: &[u8; 21] = b"last_analytics_upload";

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let mut rng = rand::thread_rng();
		let jitter_seconds = rng.gen_range(0..=ANALYTICS_JITTER);
		let startup_jitter_seconds = rng.gen_range(0..=ANALYTICS_STARTUP_JITTER);

		Ok(Arc::new(Self {
			interval: Duration::from_secs(ANALYTICS_INTERVAL),
			jitter: Duration::from_secs(jitter_seconds),
			startup_jitter: Duration::from_secs(startup_jitter_seconds),
			interrupt: Notify::new(),
			db: args.db["global"].clone(),
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				client: args.depend::<client::Service>("client"),
				config: args.depend::<config::Service>("config"),
				server_keys: args.depend::<server_keys::Service>("server_keys"),
				users: args.depend::<users::Service>("users"),
				federation: args.depend::<federation::Service>("federation"),
				server: args.server.clone(),
			},
		}))
	}

	#[tracing::instrument(skip_all, name = "analytics", level = "debug")]
	async fn worker(self: Arc<Self>) -> Result<()> {
		if !self.services.server.config.allow_analytics {
			debug!("Analytics collection is disabled");
			return Ok(());
		}

		// Send initial analytics on startup (with shorter jitter)
		tokio::time::sleep(self.startup_jitter).await;
		if let Err(e) = self.upload_analytics().await {
			warn!(%e, "Failed to upload initial analytics");
		}

		let mut i = interval(self.interval);
		i.set_missed_tick_behavior(MissedTickBehavior::Delay);
		i.reset_after(self.interval + self.jitter);

		loop {
			tokio::select! {
				() = self.interrupt.notified() => break,
				_ = i.tick() => {
					if let Err(e) = self.upload_analytics().await {
						warn!(%e, "Failed to upload analytics");
					}
				}
			}
		}

		Ok(())
	}

	fn interrupt(&self) { self.interrupt.notify_waiters(); }

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	#[tracing::instrument(skip_all)]
	async fn upload_analytics(&self) -> Result<()> {
		let payload = self.collect_metadata().await;
		let json_payload = serde_json::to_vec(&payload)?;

		// Create HTTP request
		let request = http::Request::builder()
			.method("POST")
			.uri(ANALYTICS_URL)
			.header("Content-Type", "application/json")
			.header("User-Agent", user_agent())
			.body(json_payload)?;

		// Sign the request using federation signing
		let reqwest_request = self.services.federation.sign_non_federation_request(
			ServerName::parse(ANALYTICS_SERVERNAME).unwrap(),
			request,
		)?;
		// self.sign_analytics_request(&mut request).await?;

		let response = self
			.services
			.client
			.default
			.execute(reqwest_request)
			.await?;
		let status = response.status();
		if let Ok(analytics_response) =
			serde_json::from_str::<AnalyticsResponse>(&response.text().await?)
		{
			if analytics_response.success {
				debug!("Analytics uploaded successfully");
				self.update_last_upload_timestamp().await;
			}
			let msg = analytics_response.message.unwrap_or_default();
			warn!("Analytics upload warning: {}", msg);
		} else if status.is_success() {
			info!("Analytics uploaded successfully (no structured response)");
			self.update_last_upload_timestamp().await;
		} else {
			warn!("Analytics upload failed (no structured response) with status: {}", status);
		}

		Ok(())
	}

	async fn collect_metadata(&self) -> AnalyticsPayload {
		let config = &self.services.config;

		AnalyticsPayload {
			server_name: self.services.globals.server_name().to_string(),
			version: version::version(),
			commit_hash: build_metadata::GIT_COMMIT_HASH,
			user_count: self.services.users.count().await,
			federation_enabled: config.allow_federation,
			room_creation_allowed: config.allow_room_creation,
			public_room_directory_over_federation: config
				.allow_public_room_directory_over_federation,
			build_profile: build_metadata::built::PROFILE,
			opt_level: build_metadata::built::OPT_LEVEL,
			rustc_version: build_metadata::built::RUSTC_VERSION,
			features: build_metadata::built::FEATURES.to_vec(),
			host: build_metadata::built::HOST,
			target: build_metadata::built::TARGET,
			target_arch: build_metadata::built::CFG_TARGET_ARCH,
			target_os: build_metadata::built::CFG_OS,
			target_env: build_metadata::built::CFG_ENV,
			target_family: build_metadata::built::CFG_FAMILY,
		}
	}

	async fn update_last_upload_timestamp(&self) {
		let timestamp = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap_or_default()
			.as_secs();

		self.db.raw_put(LAST_ANALYTICS_TIMESTAMP, timestamp);
	}

	pub async fn last_upload_timestamp(&self) -> u64 {
		self.db
			.get(LAST_ANALYTICS_TIMESTAMP)
			.await
			.deserialized()
			.unwrap_or(0_u64)
	}

	pub async fn force_upload(&self) -> Result<()> {
		if !self.services.config.allow_analytics {
			return Err(err!(Config("allow_analytics", "Analytics collection is disabled")));
		}

		self.upload_analytics().await
	}
}
