pub mod cache;
mod dns;
pub mod fed;

use std::sync::Arc;

use async_trait::async_trait;
use conduwuit::{Err, Result, implement};
use ipaddress::IPAddress;
use resolvematrix::server::MatrixResolver;

use self::{cache::Cache, dns::Resolver};
use crate::{Dep, client};

pub struct Service {
	pub resolver: MatrixResolver,
	pub dns: Dns,
	services: Services,
}

struct Services {
	client: Dep<client::Service>,
}

pub struct Dns {
	pub cache: Arc<Cache>,
	pub resolver: Arc<Resolver>,
}

#[async_trait]
impl crate::Service for Service {
	#[allow(clippy::as_conversions, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let cache = Cache::new(&args);
		Ok(Arc::new(Self {
			resolver: MatrixResolver::new()?,
			dns: Dns {
				cache: cache.clone(),
				resolver: Resolver::build(args.server, cache)?,
			},
			services: Services {
				client: args.depend::<client::Service>("client"),
			},
		}))
	}

	async fn clear_cache(&self) {
		// No ability to clean resolvematrix cache at the moment
		self.dns.resolver.clear_cache();
		self.dns.cache.clear().await;
	}

	fn name(&self) -> &str { crate::service::make_name(module_path!()) }
}

#[implement(Service)]
pub fn validate_ip(&self, ip: &IPAddress) -> Result<()> {
	if !self.services.client.valid_cidr_range(ip) {
		return Err!(BadServerResponse("Not allowed to send requests to this IP"));
	}

	Ok(())
}
