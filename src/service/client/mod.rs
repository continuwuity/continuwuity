mod dns_resolver;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use conduwuit::{
	Config, Result, Server, debug, err, info, trace, utils::response::LimitReadExt, warn,
};
pub(crate) use dns_resolver::DnsResolver;
use either::Either;
use hickory_resolver::{
	TokioResolver,
	config::{ConnectionConfig, NameServerConfig, ResolverConfig},
	net::runtime::TokioRuntimeProvider,
	proto::rr::IntoName,
};
use ipaddress::IPAddress;
use reqwest::{dns::Resolve, redirect};
use resolvematrix::server::{MatrixResolver, MatrixResolverBuilder};
use serde::Deserialize;

use crate::service;

pub struct Service {
	/// Regular DNS resolver
	pub dns_resolver: Arc<TokioResolver>,

	/// Tokio system resolver wrapped
	pub reqwest_resolver: Arc<DnsResolver>,

	/// Matrix server resolver according to server resolution spec
	pub matrix_resolver: Arc<MatrixResolver>,

	/// HTTP client used for fetching files during server resolution
	pub matrix_http_client: reqwest::Client,

	pub url_preview: reqwest::Client,
	pub external_resource: reqwest::Client,
	pub federation: reqwest::Client,
	pub federation_slow: reqwest::Client,
	pub sender: reqwest::Client,
	pub appservice: reqwest::Client,
	pub pusher: reqwest::Client,
	pub webpush: reqwest::Client,

	pub cidr_range_denylist: Vec<IPAddress>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let config = &args.server.config;

		// Regular DNS resolver
		let tokio_dns_resolver = get_dns_resolver(args.server)?;

		// More different DNS resolver that also implements
		// reqwest::dns::Resolve (which is required for reqwest to use it).
		// Don't ask me, I just work here. - @silversys
		let reqwest_dns_resolver = Arc::new(DnsResolver::new(tokio_dns_resolver.clone()));

		// Regular HTTP client that does regular HTTP requests with regular DNS
		// resolution, but with timeouts for /.well-known/matrix/* files
		// according to c10y config.
		let matrix_http_client = base(&args.server.config, reqwest_dns_resolver.clone())?
			.connect_timeout(Duration::from_secs(args.server.config.well_known_conn_timeout))
			.read_timeout(Duration::from_secs(args.server.config.well_known_timeout))
			.timeout(Duration::from_secs(args.server.config.well_known_timeout))
			.pool_max_idle_per_host(0)
			.redirect(redirect::Policy::limited(4))
			.build()?;

		// Resolves hostnames into ip+port or hostname+port server combinations
		// according to the server resolution spec
		let matrix_resolver = Arc::new(MatrixResolverBuilder::new()
            .dangerous_tls_accept_invalid_certs(args.server.config.allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure)
            .http_client(matrix_http_client.clone())
            .dns_resolver(tokio_dns_resolver.clone())
            .build()?);
		// Does the same as the above, but acts as a DNS resolver for reqwest
		// (implementing caching and following the matrix server resolution
		// spec)
		let matrix_dns_resolver = Arc::new(matrix_resolver.create_dns_resolver());

		let url_preview_bind_addr = config
			.url_preview_bound_interface
			.clone()
			.and_then(Either::left);

		let url_preview_bind_iface = config
			.url_preview_bound_interface
			.clone()
			.and_then(Either::right);

		let url_preview_user_agent = config
			.url_preview_user_agent
			.clone()
			.unwrap_or_else(|| conduwuit::user_agent_media().to_owned());

		Ok(Arc::new(Self {
			matrix_resolver,
			matrix_http_client,
			reqwest_resolver: reqwest_dns_resolver.clone(),

			dns_resolver: tokio_dns_resolver,

			url_preview: base(config, reqwest_dns_resolver.clone())
				.and_then(|builder| {
					builder_interface(builder, url_preview_bind_iface.as_deref())
				})?
				.local_address(url_preview_bind_addr)
				.timeout(Duration::from_secs(config.url_preview_timeout))
				.redirect(redirect::Policy::limited(3))
				.user_agent(url_preview_user_agent.replace("<version>", conduwuit::version_ua()))
				.build()?,

			external_resource: base(config, reqwest_dns_resolver.clone())?
				.redirect(redirect::Policy::limited(3))
				.build()?,

			federation: base(config, matrix_dns_resolver.clone())?
				.connect_timeout(Duration::from_secs(config.federation_conn_timeout))
				.read_timeout(Duration::from_secs(config.federation_timeout))
				.timeout(Duration::from_secs(
					config
						.federation_timeout
						.saturating_add(config.federation_conn_timeout),
				))
				.pool_max_idle_per_host(config.federation_idle_per_host.into())
				.pool_idle_timeout(Duration::from_secs(config.federation_idle_timeout))
				.redirect(redirect::Policy::limited(3))
				.build()?,

			federation_slow: base(config, matrix_dns_resolver.clone())?
				.connect_timeout(Duration::from_secs(config.federation_conn_timeout))
				.read_timeout(Duration::from_secs(config.federation_timeout.saturating_mul(6)))
				.timeout(Duration::from_secs(
					config
						.federation_timeout
						.saturating_mul(6)
						.saturating_add(config.federation_conn_timeout),
				))
				.pool_max_idle_per_host(0)
				.redirect(redirect::Policy::limited(3))
				.build()?,

			sender: base(config, matrix_dns_resolver)?
				.connect_timeout(Duration::from_secs(config.federation_conn_timeout))
				.read_timeout(Duration::from_secs(config.sender_timeout))
				.timeout(Duration::from_secs(config.sender_timeout))
				.pool_max_idle_per_host(1)
				.pool_idle_timeout(Duration::from_secs(config.sender_idle_timeout))
				.redirect(redirect::Policy::limited(2))
				.build()?,

			appservice: base(config, reqwest_dns_resolver.clone())?
				.connect_timeout(Duration::from_secs(5))
				.read_timeout(Duration::from_secs(config.appservice_timeout))
				.timeout(Duration::from_secs(config.appservice_timeout))
				.pool_max_idle_per_host(1)
				.pool_idle_timeout(Duration::from_secs(config.appservice_idle_timeout))
				.redirect(redirect::Policy::limited(2))
				.build()?,

			pusher: base(config, reqwest_dns_resolver.clone())?
				.connect_timeout(Duration::from_secs(config.pusher_conn_timeout))
				.timeout(Duration::from_secs(config.pusher_timeout))
				.pool_max_idle_per_host(1)
				.pool_idle_timeout(Duration::from_secs(config.pusher_idle_timeout))
				.redirect(redirect::Policy::limited(2))
				.build()?,

			// MSC4174 recommends against following redirects, since the
			// destination is only checked before the request is sent.
			webpush: base(config, reqwest_dns_resolver)?
				.connect_timeout(Duration::from_secs(config.pusher_conn_timeout))
				.timeout(Duration::from_secs(config.pusher_timeout))
				.pool_max_idle_per_host(1)
				.pool_idle_timeout(Duration::from_secs(config.pusher_idle_timeout))
				.redirect(redirect::Policy::none())
				.build()?,

			cidr_range_denylist: config
				.ip_range_denylist
				.iter()
				.map(IPAddress::parse)
				.inspect(|cidr| trace!("Denied CIDR range: {cidr:?}"))
				.collect::<Result<_, String>>()
				.map_err(|e| err!(Config("ip_range_denylist", e)))?,
		}))
	}

	async fn clear_cache(&self) {
		self.matrix_resolver.clear_cache();
		self.dns_resolver.clear_cache();
	}

	fn name(&self) -> &str { service::make_name(module_path!()) }
}

impl Service {
	#[inline]
	#[must_use]
	pub fn valid_cidr_range(&self, ip: &IPAddress) -> bool {
		self.cidr_range_denylist
			.iter()
			.all(|cidr| !cidr.includes(ip))
	}

	#[tracing::instrument(skip_all)]
	pub async fn recaptcha_verify(&self, secret: &str, response: &str) -> bool {
		#[derive(Deserialize, Default, Debug, Clone)]
		struct RecaptchaResponse {
			success: bool,
			#[serde(rename(deserialize = "error-codes"))]
			_error_codes: Option<Vec<String>>,
		}

		let response = match self
			.external_resource
			.post("https://www.google.com/recaptcha/api/siteverify")
			.form(&[("secret", secret), ("response", response)])
			.send()
			.await
		{
			| Ok(response) => response,
			| Err(err) => {
				warn!("ReCaptcha request failed: {err}");
				return false;
			},
		};

		let Ok(response) = response.limit_read_text(1024).await else {
			warn!("ReCaptcha response was too large??");
			return false;
		};

		match serde_json::from_str::<RecaptchaResponse>(&response) {
			| Ok(response) => response.success,
			| Err(err) => {
				warn!("ReCaptcha response failed to deserialize: {err}");
				false
			},
		}
	}
}

pub(crate) fn base(
	config: &Config,
	resolver: Arc<dyn Resolve>,
) -> Result<reqwest::ClientBuilder> {
	let mut builder = reqwest::Client::builder()
        .dns_resolver(resolver)
        .connect_timeout(Duration::from_secs(config.request_conn_timeout))
        .read_timeout(Duration::from_secs(config.request_timeout))
        .timeout(Duration::from_secs(config.request_total_timeout))
        .pool_idle_timeout(Duration::from_secs(config.request_idle_timeout))
        .pool_max_idle_per_host(config.request_idle_per_host.into())
        .user_agent(conduwuit::user_agent())
        .redirect(redirect::Policy::limited(6))
        .danger_accept_invalid_certs(config.allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure)
        .connection_verbose(cfg!(debug_assertions));

	#[cfg(feature = "gzip_compression")]
	{
		builder = if config.gzip_compression {
			builder.gzip(true)
		} else {
			builder.gzip(false).no_gzip()
		};
	};

	#[cfg(feature = "brotli_compression")]
	{
		builder = if config.brotli_compression {
			builder.brotli(true)
		} else {
			builder.brotli(false).no_brotli()
		};
	};

	#[cfg(feature = "zstd_compression")]
	{
		builder = if config.zstd_compression {
			builder.zstd(true)
		} else {
			builder.zstd(false).no_zstd()
		};
	};

	#[cfg(not(feature = "gzip_compression"))]
	{
		builder = builder.no_gzip();
	};

	#[cfg(not(feature = "brotli_compression"))]
	{
		builder = builder.no_brotli();
	};

	#[cfg(not(feature = "zstd_compression"))]
	{
		builder = builder.no_zstd();
	};

	match config.proxy.to_proxy()? {
		| Some(proxy) => Ok(builder.proxy(proxy)),
		| _ => Ok(builder),
	}
}

#[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
fn builder_interface(
	builder: reqwest::ClientBuilder,
	config: Option<&str>,
) -> Result<reqwest::ClientBuilder> {
	if let Some(iface) = config {
		Ok(builder.interface(iface))
	} else {
		Ok(builder)
	}
}

#[cfg(not(any(target_os = "android", target_os = "fuchsia", target_os = "linux")))]
fn builder_interface(
	builder: reqwest::ClientBuilder,
	config: Option<&str>,
) -> Result<reqwest::ClientBuilder> {
	use conduwuit::Err;

	if let Some(iface) = config {
		Err!("Binding to network-interface {iface:?} by name is not supported on this platform.")
	} else {
		Ok(builder)
	}
}

fn get_dns_resolver(server: &Arc<Server>) -> Result<Arc<TokioResolver>> {
	let config = &server.config;

	let (system_resolver_config, mut opts) =
		match hickory_resolver::system_conf::read_system_conf() {
			| Ok(system_conf) => system_conf,
			| Err(e) if config.dns_servers.is_empty() =>
				return Err(err!(error!("Failed to configure DNS resolver from system: {e}"))),
			| Err(e) => {
				warn!("Failed to read system DNS configuration, using dns_servers: {e}");
				Default::default()
			},
		};

	let mut conf = ResolverConfig::from_name_servers(vec![]);

	// Set domain and search domains from system config (/etc/resolv.conf)
	if let Some(domain) = system_resolver_config.domain() {
		conf.set_domain(domain.clone());
	}
	for sys_conf in system_resolver_config.search() {
		conf.add_search(sys_conf.clone());
	}

	// Read nameservers from c10y config file
	let config_nameservers: Vec<NameServerConfig> = server
		.config
		.dns_servers
		.iter()
		.filter_map(IntoName::to_ip)
		.map(NameServerConfig::udp_and_tcp)
		.collect();
	let system_nameservers = system_resolver_config.name_servers().to_vec();

	let nameservers = if !config_nameservers.is_empty() {
		debug!("Using DNS nameservers from config file");
		config_nameservers
	} else {
		debug!("Using DNS nameservers from /etc/resolv.conf");
		system_nameservers
	};
	info!(?nameservers, "Resolved nameservers for DNS client");

	for sys_conf in nameservers {
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
	opts.attempts = config.dns_attempts.into();
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

	Ok(resolver)
}
