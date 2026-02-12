use std::{ops::Deref, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use conduwuit::{
	Result, Server,
	config::{Config, check},
	error, implement,
};

use crate::registration_tokens::{ValidToken, ValidTokenSource};

pub struct Service {
	server: Arc<Server>,
}

const SIGNAL: &str = "SIGUSR1";

impl Service {
	/// Get the registration token set in the config file, if it exists.
	#[must_use]
	pub fn get_config_file_token(&self) -> Option<ValidToken> {
		self.registration_token.clone().map(|token| ValidToken {
			token,
			source: ValidTokenSource::ConfigFile,
		})
	}
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self { server: args.server.clone() }))
	}

	async fn worker(self: Arc<Self>) -> Result {
		while self.server.running() {
			if self.server.signal.subscribe().recv().await == Ok(SIGNAL) {
				if let Err(e) = self.handle_reload() {
					error!("Failed to reload config: {e}");
				}
			}
		}

		Ok(())
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Deref for Service {
	type Target = Arc<Config>;

	#[inline]
	fn deref(&self) -> &Self::Target { &self.server.config }
}

#[implement(Service)]
fn handle_reload(&self) -> Result {
	if self.server.config.config_reload_signal {
		#[cfg(all(feature = "systemd", target_os = "linux"))]
		sd_notify::notify(false, &[
			sd_notify::NotifyState::Reloading,
			sd_notify::NotifyState::monotonic_usec_now().expect("Failed to read monotonic time"),
		])
		.expect("failed to notify systemd of reloading state");

		let config_paths = self.server.config.config_paths.clone().unwrap_or_default();
		self.reload(&config_paths)?;

		#[cfg(all(feature = "systemd", target_os = "linux"))]
		sd_notify::notify(false, &[sd_notify::NotifyState::Ready])
			.expect("failed to notify systemd of ready state");
	}

	Ok(())
}

#[implement(Service)]
pub fn reload(&self, paths: &[PathBuf]) -> Result<Arc<Config>> {
	let old = self.server.config.clone();
	let new = Config::load(paths).and_then(|raw| Config::new(&raw))?;

	check::reload(&old, &new)?;
	self.server.config.update(new)
}
