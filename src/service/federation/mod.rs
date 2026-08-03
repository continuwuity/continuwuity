mod execute;

use std::{collections::HashMap, sync::Arc, time::Duration};

use conduwuit::{
	Result, Server, SyncRwLock,
	utils::{millis_since_unix_epoch, time::exponential_backoff::min_exp_backoff_duration},
};
pub(crate) use execute::FederationPathBuilderInput;
use ruma::{OwnedServerName, ServerName};

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
		// TODO(nex): Can multiple concurrent failures cause this health monitor to
		// rapidly max out?
		//
		// consider: a profile query and key claim query both go out at the same time,
		// and both fail. They both then go to hit this (synchronous) function, which
		// means one of them will increment the failure count by 1, and consequently
		// increase the exp backoff. Then the second failure is allowed to call the
		// function, at which point it increments the failure count AGAIN, increasing
		// the backoff again.
		// Technically, these are two distinct failures. However, from a UX perspective,
		// they happened at the same time, so shouldn't incur a double penalty?
		// Perhaps only incrementing the retry counter when the current next_retry is in
		// the past might help.
		//
		// You know it's a banger thought process when the comment is longer than the
		// code itself.
		let unix_now = millis_since_unix_epoch();
		let mut map = self.remote_health.write();
		let (retries, next_retry) = map.entry(server_name).or_default();

		let min = self.services.server.config.sender_timeout;
		let max = self.services.server.config.sender_retry_backoff_limit;

		*retries = retries.saturating_add(1);
		*next_retry = unix_now.saturating_add(
			u64::try_from(min_exp_backoff_duration(min, max, *retries).as_millis())
				.expect("backoff milliseconds should not exceed u64::MAX"),
		);
	}

	/// Marks a server as "healthy" by removing it from the health map.
	///
	/// TODO: flush senders too
	pub fn mark_healthy(&self, server_name: &ServerName) {
		// TODO: We need to make sure the sender flush DOESN'T trigger if this is called
		// by the senders themselves.
		let mut map = self.remote_health.write();
		map.remove(server_name);
	}
}
