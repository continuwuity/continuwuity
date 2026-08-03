mod execute;

use std::{collections::HashMap, sync::Arc, time::Duration};

use assign::assign;
use async_trait::async_trait;
use conduwuit::{
	Error, Result, Server, SyncRwLock, debug,
	utils::{millis_since_unix_epoch, time::exponential_backoff::min_exp_backoff_duration},
};
pub(crate) use execute::FederationPathBuilderInput;
use http::StatusCode;
use ruma::{
	OwnedServerName, ServerName,
	api::error::{ErrorKind, LimitExceededErrorData, RetryAfter},
};

use crate::{Dep, client, moderation, server_keys};

pub struct Service {
	services: Services,
	remote_health: SyncRwLock<HashMap<OwnedServerName, (u32, u64)>>,
}

struct Services {
	server: Arc<Server>,
	client: Dep<client::Service>,
	server_keys: Dep<server_keys::Service>,
	moderation: Dep<moderation::Service>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				server: args.server.clone(),
				client: args.depend::<client::Service>("client"),
				server_keys: args.depend::<server_keys::Service>("server_keys"),
				moderation: args.depend::<moderation::Service>("moderation"),
			},
			remote_health: SyncRwLock::new(HashMap::new()),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }

	async fn clear_cache(&self) {
		let mut map = self.remote_health.write();
		map.clear();
	}
}

impl Service {
	/// Checks if a remote is "healthy". "Healthy" is defined by either:
	///
	/// * The remote has not been marked as having a failed request, OR
	/// * The next retry timestamp is in the past
	pub fn is_healthy(&self, server_name: &ServerName) -> bool {
		let map = self.remote_health.read();
		let unix_now = millis_since_unix_epoch();
		if let Some((_, next_retry)) = map.get(server_name) {
			unix_now >= *next_retry
		} else {
			true
		}
	}

	/// Returns how long the server should wait before attempting to contact the
	/// remote again.
	pub fn retry_after(&self, server_name: &ServerName) -> Option<Duration> {
		let map = self.remote_health.read();
		let unix_now = millis_since_unix_epoch();
		map.get(server_name)
			.map(|(_, next_retry)| Duration::from_millis(*next_retry - unix_now))
	}

	/// Marks or updates a remote's health status as unhealthy. If the remote is
	/// not already marked as unhealthy, a new entry is created. Otherwise, the
	/// retry count is incremented and
	pub fn hit_unhealthy(&self, server_name: OwnedServerName) {
		let unix_now = millis_since_unix_epoch();
		let mut map = self.remote_health.write();
		let sn2 = server_name.clone(); // for logging since map.entry() moves
		let (retries, next_retry) = map.entry(server_name).or_default();
		if *next_retry > unix_now {
			// Don't update the retry marker if we are already in a backoff
			// period. This prevents the backoff skyrocketing if multiple
			// concurrent or closely-related requests fail and consequently try
			// to mark as offline.
			return;
		}

		let min = self.services.server.config.sender_timeout;
		let max = self.services.server.config.sender_retry_backoff_limit;

		*retries = retries.saturating_add(1);
		*next_retry = unix_now.saturating_add(
			u64::try_from(min_exp_backoff_duration(min, max, *retries).as_millis())
				.expect("backoff milliseconds should not exceed u64::MAX"),
		);
		debug!(
			"{} is (now) unhealthy ({} retries, blocked until: {})",
			sn2, *retries, *next_retry
		);
	}

	/// Marks a server as "healthy" by removing it from the health map.
	///
	/// TODO: flush senders too
	pub fn mark_healthy(&self, server_name: &ServerName) {
		// TODO: We need to make sure the sender flush DOESN'T trigger if this is called
		// by the senders themselves.
		let mut map = self.remote_health.write();
		if map.remove(server_name).is_some() {
			debug!("{} is now healthy", server_name);
		}
	}

	/// Returns a rate-limited error if the remote is unhealthy.
	fn ensure_remote_is_healthy(&self, server_name: &ServerName) -> Result<()> {
		if self.is_healthy(server_name) {
			Ok(())
		} else {
			let retry_after = self
				.retry_after(server_name)
				.expect("remote is unhealthy and must have an accompanying retry timestamp");
			Err(Error::Request(
				ErrorKind::LimitExceeded(assign!(LimitExceededErrorData::new(), {
					retry_after: Some(RetryAfter::Delay(retry_after)),
				})),
				format!(
					"Remote server is currently unhealthy (not retrying for another {} seconds)",
					retry_after.as_secs()
				)
				.into(),
				StatusCode::TOO_MANY_REQUESTS,
			))
		}
	}
}
