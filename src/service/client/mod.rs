mod dns;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use conduwuit::{Config, Result, err, trace};
use either::Either;
use ipaddress::IPAddress;
use reqwest::redirect;
use resolvematrix::server::{MatrixResolver, MatrixResolverBuilder};

use crate::{client::dns::Resolver, service};

pub struct Service {
	pub resolver: MatrixResolver,
	pub dns: Arc<Resolver>,

	pub default: reqwest::Client,
	pub url_preview: reqwest::Client,
	pub extern_media: reqwest::Client,
	pub federation: reqwest::Client,
	pub federation_slow: reqwest::Client,
	pub sender: reqwest::Client,
	pub appservice: reqwest::Client,
	pub pusher: reqwest::Client,

	pub cidr_range_denylist: Vec<IPAddress>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let config = &args.server.config;
		let dns = Resolver::build(args.server)?;
		let resolver = MatrixResolverBuilder::new()
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
			.dns_resolver(dns.resolver.clone())
			.build()?;

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
			resolver,
			dns: dns.clone(),

			default: base(config)?.dns_resolver(dns.clone()).build()?,

			url_preview: base(config)
				.and_then(|builder| {
					builder_interface(builder, url_preview_bind_iface.as_deref())
				})?
				.local_address(url_preview_bind_addr)
				.dns_resolver(dns.clone())
				.timeout(Duration::from_secs(config.url_preview_timeout))
				.redirect(redirect::Policy::limited(3))
				.user_agent(url_preview_user_agent)
				.build()?,

			extern_media: base(config)?
				.dns_resolver(dns.clone())
				.redirect(redirect::Policy::limited(3))
				.build()?,

			federation: base(config)?
				.dns_resolver(dns.clone())
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

			federation_slow: base(config)?
				.dns_resolver(dns.clone())
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

			sender: base(config)?
				.dns_resolver(dns.clone())
				.connect_timeout(Duration::from_secs(config.federation_conn_timeout))
				.read_timeout(Duration::from_secs(config.sender_timeout))
				.timeout(Duration::from_secs(config.sender_timeout))
				.pool_max_idle_per_host(1)
				.pool_idle_timeout(Duration::from_secs(config.sender_idle_timeout))
				.redirect(redirect::Policy::limited(2))
				.build()?,

			appservice: base(config)?
				.dns_resolver(dns.clone())
				.connect_timeout(Duration::from_secs(5))
				.read_timeout(Duration::from_secs(config.appservice_timeout))
				.timeout(Duration::from_secs(config.appservice_timeout))
				.pool_max_idle_per_host(1)
				.pool_idle_timeout(Duration::from_secs(config.appservice_idle_timeout))
				.redirect(redirect::Policy::limited(2))
				.build()?,

			pusher: base(config)?
				.dns_resolver(dns.clone())
				.connect_timeout(Duration::from_secs(config.pusher_conn_timeout))
				.timeout(Duration::from_secs(config.pusher_timeout))
				.pool_max_idle_per_host(1)
				.pool_idle_timeout(Duration::from_secs(config.pusher_idle_timeout))
				.redirect(redirect::Policy::limited(2))
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
		self.resolver.clear_cache();
		self.dns.resolver.clear_cache();
	}

	fn name(&self) -> &str { service::make_name(std::module_path!()) }
}

impl Service {
	#[inline]
	#[must_use]
	pub fn valid_cidr_range(&self, ip: &IPAddress) -> bool {
		self.cidr_range_denylist
			.iter()
			.all(|cidr| !cidr.includes(ip))
	}
}

pub fn base(config: &Config) -> Result<reqwest::ClientBuilder> {
	let mut builder = reqwest::Client::builder()
		.hickory_dns(true)
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
