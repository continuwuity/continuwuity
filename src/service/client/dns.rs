use std::{net::SocketAddr, sync::Arc, time::Duration};

use conduwuit::{Result, Server, err};
use futures::FutureExt;
use hickory_resolver::{
	TokioResolver, config::ConnectionConfig, lookup_ip::LookupIp,
	net::runtime::TokioRuntimeProvider,
};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

pub struct Resolver {
	pub(crate) resolver: Arc<TokioResolver>,
	server: Arc<Server>,
}

type ResolvingResult = Result<Addrs, Box<dyn std::error::Error + Send + Sync>>;

impl Resolver {
	#[allow(clippy::as_conversions, clippy::cast_sign_loss, clippy::cast_possible_truncation)]
	pub(super) fn build(server: &Arc<Server>) -> Result<Arc<Self>> {
		let config = &server.config;
		let (sys_conf, mut opts) = hickory_resolver::system_conf::read_system_conf()
			.map_err(|e| err!(error!("Failed to configure DNS resolver from system: {e}")))?;

		let mut conf = hickory_resolver::config::ResolverConfig::default();

		if let Some(domain) = sys_conf.domain() {
			conf.set_domain(domain.clone());
		}

		for sys_conf in sys_conf.search() {
			conf.add_search(sys_conf.clone());
		}

		for sys_conf in sys_conf.name_servers() {
			let mut ns = sys_conf.clone();

			if config.query_over_tcp_only {
				ns.connections = vec![ConnectionConfig::tcp()];
			}

			ns.trust_negative_responses = !config.query_all_nameservers;

			conf.add_name_server(ns);
		}

		opts.cache_size = u64::from(config.dns_cache_entries);
		opts.preserve_intermediates = true;
		opts.negative_min_ttl = Some(Duration::from_secs(config.dns_min_ttl_nxdomain));
		opts.negative_max_ttl = Some(Duration::from_hours(720));
		opts.positive_min_ttl = Some(Duration::from_secs(config.dns_min_ttl));
		opts.positive_max_ttl = Some(Duration::from_hours(168));
		opts.timeout = Duration::from_secs(config.dns_timeout);
		opts.attempts = config.dns_attempts as usize;
		opts.try_tcp_on_error = config.dns_tcp_fallback;
		opts.num_concurrent_reqs = 1;
		opts.edns0 = true;
		opts.case_randomization = true;
		opts.ip_strategy = match config.ip_lookup_strategy {
			| 1 => hickory_resolver::config::LookupIpStrategy::Ipv4Only,
			| 2 => hickory_resolver::config::LookupIpStrategy::Ipv6Only,
			| 3 => hickory_resolver::config::LookupIpStrategy::Ipv4AndIpv6,
			| 4 => hickory_resolver::config::LookupIpStrategy::Ipv6thenIpv4,
			| _ => hickory_resolver::config::LookupIpStrategy::Ipv4thenIpv6,
		};

		let runtime_provider = TokioRuntimeProvider::new();
		let mut builder = TokioResolver::builder_with_config(conf, runtime_provider);
		*builder.options_mut() = opts;
		let resolver = Arc::new(builder.build().expect("failed to build resolver :("));

		Ok(Arc::new(Self {
			resolver: resolver.clone(),
			server: server.clone(),
		}))
	}

	/// Clear the in-memory hickory-dns caches
	#[inline]
	pub fn clear_cache(&self) { self.resolver.clear_cache(); }
}

impl Resolve for Resolver {
	fn resolve(&self, name: Name) -> Resolving {
		resolve_to_reqwest(self.server.clone(), self.resolver.clone(), name).boxed()
	}
}

async fn resolve_to_reqwest(
	server: Arc<Server>,
	resolver: Arc<TokioResolver>,
	name: Name,
) -> ResolvingResult {
	use std::{io, io::ErrorKind::Interrupted};

	let handle_shutdown = || Box::new(io::Error::new(Interrupted, "Server shutting down"));
	let handle_results = |results: LookupIp| {
		Box::new(
			results
				.iter()
				.collect::<Vec<_>>()
				.into_iter()
				.map(|ip| SocketAddr::new(ip, 0)),
		)
	};

	tokio::select! {
		results = resolver.lookup_ip(name.as_str()) => Ok(handle_results(results?)),
		() = server.until_shutdown() => Err(handle_shutdown()),
	}
}
