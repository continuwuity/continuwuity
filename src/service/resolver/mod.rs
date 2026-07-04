pub mod cache;
mod dns;
pub mod fed;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use conduwuit::Result;
use reqwest::redirect;
use resolvematrix::server::{MatrixResolver, MatrixResolverBuilder};

use self::{cache::Cache, dns::Resolver};
use crate::client::base;

pub struct Service {
	pub resolver: MatrixResolver,
	pub dns: Dns,
	#[allow(dead_code)] // This service doesn't access services after construction
	services: Services,
}

struct Services;

pub struct Dns {
	pub cache: Arc<Cache>,
	pub resolver: Arc<Resolver>,
}

#[async_trait]
impl crate::Service for Service {
	#[allow(clippy::as_conversions, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let cache = Cache::new(&args);
		let resolver = Resolver::build(args.server, cache.clone())?;
		Ok(Arc::new(Self {
			resolver: MatrixResolverBuilder::new()
				.dangerous_tls_accept_invalid_certs(args.server.config.allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure)
				.http_client(
					base(&args.server.config)?
						.connect_timeout(Duration::from_secs(args.server.config.well_known_conn_timeout))
						.read_timeout(Duration::from_secs(args.server.config.well_known_timeout))
						.timeout(Duration::from_secs(args.server.config.well_known_timeout))
						.pool_max_idle_per_host(0)
						.redirect(redirect::Policy::limited(4))
						.build()?
				)
				.dns_resolver(resolver.resolver.clone())
				.build()?,
			dns: Dns {
				cache,
				resolver,
			},
			services: Services {},
		}))
	}

	async fn clear_cache(&self) {
		self.resolver.clear_cache();
		self.dns.resolver.clear_cache();
		self.dns.cache.clear().await;
	}

	fn name(&self) -> &str { crate::service::make_name(module_path!()) }
}
