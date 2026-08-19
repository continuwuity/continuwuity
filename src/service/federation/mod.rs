mod execute;

use std::{
	collections::{HashMap, HashSet},
	sync::Arc,
	time::Duration,
};

use assign::assign;
use async_trait::async_trait;
use conduwuit::{Error, Result, Server, SyncRwLock, debug, utils::millis_since_unix_epoch};
pub(crate) use execute::FederationPathBuilderInput;
use http::StatusCode;
use ruma::{
	OwnedServerName, ServerName,
	api::error::{ErrorKind, LimitExceededErrorData, RetryAfter},
};

use crate::{Dep, client, moderation, server_keys};

pub struct Service {
	services: Services,
	pub remote_health: SyncRwLock<HashMap<OwnedServerName, (u32, u64)>>,
	pub stale_destinations: SyncRwLock<HashSet<OwnedServerName>>,
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
			stale_destinations: SyncRwLock::new(HashSet::new()),
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
			.map(|(_, next_retry)| Duration::from_millis((*next_retry).saturating_sub(unix_now)))
	}

	/// Marks or updates a remote's health status as unhealthy. If the remote is
	/// not already marked as unhealthy, a new entry is created. Otherwise, the
	/// retry count is incremented and next retry window is updated.
	///
	/// Does not update the marker if the backoff period is already in effect.
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

		let min = Duration::from_secs(self.services.server.config.sender_retry_backoff_base);
		let max = Duration::from_secs(self.services.server.config.sender_retry_backoff_limit);

		*retries = retries.saturating_add(1);
		let next_interval = min.saturating_mul(*retries).min(max);
		*next_retry = unix_now.saturating_add(
			u64::try_from(next_interval.as_millis())
				.expect("backoff milliseconds should not exceed u64::MAX"),
		);
		debug!(
			"{} is (now) unhealthy ({} retries, blocked until: {})",
			sn2, *retries, *next_retry
		);
	}

	/// Marks a server as "healthy" by removing it from the health map.
	///
	/// Returns true if the server was previously marked as unhealthy, false
	/// otherwise.
	pub fn mark_healthy(&self, server_name: &ServerName) -> bool {
		let mut health_map = self.remote_health.write();
		let was_unhealthy = health_map.remove(server_name).is_some();
		if was_unhealthy {
			debug!("{} is now healthy", server_name);
		}
		// The lock for remote_health is deliberately retained until the end of the
		// function to prevent parallel requests from marking the server as healthy
		// and then immediately marking it as unhealthy again due to the stale cache
		// we might be about to clear
		let mut stale_destinations = self.stale_destinations.write();
		if stale_destinations.remove(server_name) {
			debug!(
				"{server_name} is no longer unhealthy but was stale, clearing destination cache \
				 entry"
			);
			self.services
				.client
				.matrix_resolver
				.remove_cache_entry(server_name.as_str());
		}

		was_unhealthy
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
					"Remote server {} is currently unhealthy (not retrying for another {} \
					 seconds)",
					server_name,
					retry_after.as_secs()
				)
				.into(),
				StatusCode::TOO_MANY_REQUESTS,
			))
		}
	}

	/// Marks a destination as stale, which will cause the destination cache to
	/// be invalidated next time we receive a request FROM that destination.
	/// This does not inherently mark the remote as "unhealthy".
	///
	/// Typically, this should only be done if a connection error is
	/// encountered, which might indicate that the address of the destination
	/// is incorrect or has since moved.
	pub fn mark_destination_stale(&self, server_name: &ServerName) {
		self.stale_destinations
			.write()
			.insert(server_name.to_owned());
	}

	/// Determines whether a destination should be marked "stale" depending on
	/// the returned error.
	pub fn should_mark_stale(&self, error: &Error) -> bool {
		if let Error::Reqwest(error) = error {
			if error.is_connect() {
				return true;
			}
		}

		match error.status_code() {
			// Some special servers account for this specifically
			| StatusCode::MISDIRECTED_REQUEST
			// Common error codes observed for misdirected requests
			// | StatusCode::NOT_FOUND  This one can be encountered naturally
			| StatusCode::METHOD_NOT_ALLOWED
			| StatusCode::IM_A_TEAPOT => true,
			_ => false,
		}
	}

	/// Returns a clone of the internal remote health tracking map.
	pub fn remote_health(&self) -> HashMap<OwnedServerName, (u32, u64)> {
		self.remote_health.read().clone()
	}

	/// Returns a clone of the internal stale destinations set.
	pub fn stale_destinations(&self) -> HashSet<OwnedServerName> {
		self.stale_destinations.read().clone()
	}
}
