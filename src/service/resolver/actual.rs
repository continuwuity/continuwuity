use std::fmt::Debug;

use conduwuit::{Err, Result, debug, debug_info, err, error, trace};
use futures::{FutureExt, TryFutureExt};
use hickory_resolver::{
	net::{DnsError, NetError},
	proto::rr::rdata::SRV,
};
use ipaddress::IPAddress;
use ruma::ServerName;

use super::{
	cache::{CachedDest, CachedOverride, MAX_IPS},
	fed::{FedDest, PortString, add_port_to_hostname, ensure_host_has_port, get_ip_with_port},
};

const DEFAULT_PORT: u16 = 8448;

#[derive(Clone, Debug)]
pub(crate) struct ActualDest {
	pub(crate) dest: FedDest,
	pub(crate) host: String,
}

impl ActualDest {
	#[inline]
	pub(crate) fn string(&self) -> String { self.dest.https_string() }
}

impl super::Service {
	#[tracing::instrument(skip_all, level = "debug", name = "resolve")]
	pub(crate) async fn get_actual_dest(&self, server_name: &ServerName) -> Result<ActualDest> {
		let (CachedDest { dest, host, .. }, _cached) =
			self.lookup_actual_dest(server_name).await?;

		Ok(ActualDest { dest, host })
	}

	pub(crate) async fn lookup_actual_dest(
		&self,
		server_name: &ServerName,
	) -> Result<(CachedDest, bool)> {
		if let Ok(result) = self.cache.get_destination(server_name).await {
			return Ok((result, true));
		}

		let _dedup = self.resolving.lock(server_name.as_str());
		if let Ok(result) = self.cache.get_destination(server_name).await {
			return Ok((result, true));
		}

		self.resolve_actual_dest(server_name, true)
			.inspect_ok(|result| self.cache.set_destination(server_name, result))
			.map_ok(|result| (result, false))
			.boxed()
			.await
	}

	/// Returns: `actual_destination` + `host` variable used for logging
	#[tracing::instrument(name = "actual", level = "debug", skip(self, cache))]
	pub async fn resolve_actual_dest(
		&self,
		dest: &ServerName,
		cache: bool,
	) -> Result<CachedDest> {
		debug!(
			dest = %dest,
			cache = %cache,
			"Resolving server name and port"
		);
		// Ensure dest is a valid connection endpoint
		self.validate_dest(dest)?;

		// Clippy believes this can be a clone, however we are actually converting
		// ServerName to String
		#[allow(clippy::implicit_clone)]
		let mut host = dest.to_string().to_owned();
		let actual_dest = self.resolve_server_name(dest, cache, &mut host).await?;

		host = ensure_host_has_port(&host).to_string();

		debug!(
			dest = %dest,
			actual_dest = %actual_dest,
			host = %host,
			"Finished resolving server name"
		);
		Ok(CachedDest {
			dest: actual_dest,
			host,
			expire: CachedDest::default_expire(),
		})
	}

	/// Performs the server resolution steps as per the specification:
	/// <https://matrix.org/docs/spec/server_server/r0.1.4#resolving-server-names>
	async fn resolve_server_name(
		&self,
		dest: &ServerName,
		cache: bool,
		host: &mut String,
	) -> Result<FedDest> {
		// 1. If `dest` is an IP, use it directly. If a port is provided as well
		//    (IP:port socket pair) use that, otherwise default to port 8448
		if let Some(fed_dest) = get_ip_with_port(dest.as_str()) {
			debug!("1: IP literal with provided or default port");
			return Ok(fed_dest);
		}

		// 2. If `dest` is a hostname and has a provided port (format of `host:port`),
		//    resolve the hostname to an IP address and connect it and the provided port
		if dest.as_str().contains(':') {
			return self.resolve_2_host_port(dest, cache).await;
		}

		// Pre-resolve IP? Unsure what overrides exactly do, system is due to be removed
		// either way https://matrix.to/#/!da26JtAjE6APGLnX8ncWsvc-skF2KQZ9Nw_MbNpYD2k/%24_hq6JP0JXANbMTMPdV64iZbgbsZdhy92M5ndDYGy6No
		self.conditional_query_and_cache(dest.as_str(), DEFAULT_PORT, true)
			.await?;

		// Ensure server is running (not shutting down) before continuing resolution
		self.services.server.check_running()?;

		// 3. If `dest` is a hostname with no port, send GET to `https://<dest>/.well-known/matrix/server`.
		// If invalid JSON (throws error), skip to step 4. Otherwise, parse `delegated`
		// as `<hostname>[:<port>]` and...
		if let Some(delegated) = self.request_well_known(dest.as_str()).await? {
			return self.resolve_3_well_known(host, cache, delegated).await;
		}

		// 4. if .well-known errored, perform SRV (see 3.3)
		if let Some(overrider) = self.query_srv_record(dest.as_str()).await? {
			return self.resolve_4_srv_lookup(host, cache, overrider).await;
		}

		// 5. if .well-known errored and no SRV exists, resolve IP and connect on
		//    default port (8448)
		self.resolve_5_direct(dest, cache).await
	}

	/// Parse a host:port socket pair into separate parts, and resolve the
	/// hostname into an IP address
	async fn resolve_2_host_port(&self, dest: &ServerName, cache: bool) -> Result<FedDest> {
		debug!("2: Hostname with included port");
		let (host, port) = dest.as_str().split_once(':').unwrap();

		self.conditional_query_and_cache(
			host,
			port.parse::<u16>().unwrap_or(DEFAULT_PORT),
			cache,
		)
		.await?;

		Ok(FedDest::Named(
			host.to_owned(),
			port.try_into().unwrap_or_else(|_| FedDest::default_port()),
		))
	}

	async fn resolve_3_well_known(
		&self,
		host: &mut String,
		cache: bool,
		delegated: String,
	) -> Result<FedDest> {
		debug!("3: A .well-known file is available");
		*host = add_port_to_hostname(&delegated).uri_string();

		// 3.1 - If <delegated> is IP:port, connect to that,
		//       or IP with default port if no port provided (8448)
		if let Some(host_and_port) = get_ip_with_port(&delegated) {
			debug!("3.1: IP with port in .well-known file");
			return Ok(host_and_port);
		}

		// 3.2 - If <delegated> is hostname:port, lookup IP for hostname and connect
		if let Some(pos) = &delegated.find(':') {
			return self
				.resolve_3_2_hostname_port(cache, &delegated, *pos)
				.await;
		}

		// 3.3 - If <delegated> is not an IP and there is no port, lookup SRV
		// `_matrix._tcp.<delegated>` (which may provide a new hostname + port to use,
		// see steps 3.1 and 3.2)
		trace!("Delegated hostname has no port, querying SRV");
		if let Some(overrider) = self.query_srv_record(&delegated).await? {
			return self.resolve_3_3_use_srv(cache, &delegated, overrider).await;
		}

		self.resolve_3_4_use_default_port(cache, delegated).await
	}

	async fn resolve_3_2_hostname_port(
		&self,
		cache: bool,
		delegated: &str,
		pos: usize,
	) -> Result<FedDest> {
		debug!("3.2: Hostname with port in .well-known file");
		let (host, port) = &delegated.split_at(pos);
		self.conditional_query_and_cache(
			host,
			port.parse::<u16>().unwrap_or(DEFAULT_PORT),
			cache,
		)
		.await?;

		trace!("Successfully resolved IP for {delegated}");
		Ok(FedDest::Named(
			host.to_owned().to_owned(),
			port.to_owned()
				.try_into()
				.unwrap_or_else(|_| FedDest::default_port()),
		))
	}

	async fn resolve_3_3_use_srv(
		&self,
		cache: bool,
		delegated: &String,
		overrider: FedDest,
	) -> Result<FedDest> {
		debug!("3.3: SRV lookup successful");

		let force_port = overrider.port();
		self.conditional_query_and_cache_override(
			delegated,
			&overrider.hostname(),
			force_port.unwrap_or(DEFAULT_PORT),
			cache,
		)
		.await?;

		if let Some(port) = force_port {
			return Ok(FedDest::Named(
				delegated.to_owned(),
				format!(":{port}")
					.as_str()
					.try_into()
					.unwrap_or_else(|_| FedDest::default_port()),
			));
		}

		Ok(add_port_to_hostname(delegated))
	}

	async fn resolve_3_4_use_default_port(
		&self,
		cache: bool,
		delegated: String,
	) -> Result<FedDest> {
		debug!("3.4: No SRV records found, use the hostname from .well-known with default port");
		self.conditional_query_and_cache(&delegated, DEFAULT_PORT, cache)
			.await?;
		Ok(add_port_to_hostname(&delegated))
	}

	async fn resolve_4_srv_lookup(
		&self,
		host: &str,
		cache: bool,
		overrider: FedDest,
	) -> Result<FedDest> {
		debug!("4: No .well-known; SRV record found");
		let force_port = overrider.port();
		self.conditional_query_and_cache_override(
			host,
			&overrider.hostname(),
			force_port.unwrap_or(DEFAULT_PORT),
			cache,
		)
		.await?;

		if let Some(port) = force_port {
			let port = format!(":{port}");

			return Ok(FedDest::Named(
				host.to_owned(),
				PortString::from(port.as_str()).unwrap_or_else(|_| FedDest::default_port()),
			));
		}

		Ok(add_port_to_hostname(host))
	}

	async fn resolve_5_direct(&self, dest: &ServerName, cache: bool) -> Result<FedDest> {
		debug!("5: No port provided and no SRV record found");
		self.conditional_query_and_cache(dest.as_str(), DEFAULT_PORT, cache)
			.await?;

		Ok(add_port_to_hostname(dest.as_str()))
	}

	#[inline]
	async fn conditional_query_and_cache(
		&self,
		hostname: &str,
		port: u16,
		cache: bool,
	) -> Result {
		self.conditional_query_and_cache_override(hostname, hostname, port, cache)
			.await
	}

	#[inline]
	async fn conditional_query_and_cache_override(
		&self,
		untername: &str,
		hostname: &str,
		port: u16,
		cache: bool,
	) -> Result {
		if !cache {
			return Ok(());
		}

		if self.cache.has_override(untername).await {
			return Ok(());
		}

		self.query_and_cache_override(untername, hostname, port)
			.await
	}

	#[tracing::instrument(name = "ip", level = "debug", skip(self))]
	async fn query_and_cache_override(
		&self,
		untername: &'_ str,
		hostname: &'_ str,
		port: u16,
	) -> Result {
		self.services.server.check_running()?;

		debug!("querying IP for {untername:?} ({hostname:?}:{port})");
		match self.resolver.resolver.lookup_ip(hostname.to_owned()).await {
			| Err(e) => Self::handle_resolve_error(&e, hostname),
			| Ok(override_ip) => {
				self.cache.set_override(untername, &CachedOverride {
					ips: override_ip.into_iter().take(MAX_IPS).collect(),
					port,
					expire: CachedOverride::default_expire(),
					overriding: (hostname != untername)
						.then_some(hostname.into())
						.inspect(|_| debug_info!("{untername:?} overridden by {hostname:?}")),
				});

				Ok(())
			},
		}
	}

	#[tracing::instrument(name = "srv", level = "debug", skip(self))]
	async fn query_srv_record(&self, hostname: &'_ str) -> Result<Option<FedDest>> {
		let hostnames =
			[format!("_matrix-fed._tcp.{hostname}."), format!("_matrix._tcp.{hostname}.")];

		for hostname in hostnames {
			self.services.server.check_running()?;

			debug!("querying SRV for {hostname:?}");
			let hostname = hostname.trim_end_matches('.');
			match self.resolver.resolver.srv_lookup(hostname).await {
				| Err(e) => Self::handle_resolve_error(&e, hostname)?,
				| Ok(result) => {
					return Ok(result.answers().iter().next().map(|result| {
						let data = result.try_borrow::<SRV>().expect("should be SRV response");

						FedDest::Named(
							data.data()
								.target
								.to_string()
								.trim_end_matches('.')
								.to_owned(),
							format!(":{}", data.data().port)
								.as_str()
								.try_into()
								.unwrap_or_else(|_| FedDest::default_port()),
						)
					}));
				},
			}
		}

		Ok(None)
	}

	fn handle_resolve_error(err: &NetError, host: &'_ str) -> Result<()> {
		match err {
			| NetError::NoConnections => {
				error!(
					"Your DNS server is overloaded and has ran out of connections. It is \
					 strongly recommended you remediate this issue to ensure proper federation \
					 connectivity."
				);

				Err!(error!(%host, "DNS error: {err}"))
			},
			| NetError::Timeout => Err!(error!(%host, "DNS query timed out")),
			| NetError::Dns(DnsError::NoRecordsFound(..)) => {
				// Raise to debug_warn if we can find out the result wasn't from cache
				debug!(%host, "No DNS records found: {err}");
				Ok(())
			},
			| _ => Err!(error!(%host, "DNS error: {err}")),
		}
	}

	/// Ensure `dest` is a valid destination (valid ip if it is an IP), and not
	/// ourselves (unless in config)
	fn validate_dest(&self, dest: &ServerName) -> Result<()> {
		if dest == self.services.server.name && !self.services.server.config.federation_loopback {
			return Err!("Won't send federation request to ourselves");
		}

		if dest.is_ip_literal() || IPAddress::is_valid(dest.host()) {
			self.validate_dest_ip_literal(dest)?;
		}

		debug!(dest = %dest, "Valid destination for resolution");
		Ok(())
	}

	fn validate_dest_ip_literal(&self, dest: &ServerName) -> Result<()> {
		trace!("Destination is an IP literal, checking against IP range denylist.",);
		debug_assert!(
			dest.is_ip_literal() || !IPAddress::is_valid(dest.host()),
			"Destination is not an IP literal."
		);
		let ip = IPAddress::parse(dest.host()).map_err(|e| {
			err!(BadServerResponse(debug_error!("Failed to parse IP literal from string: {e}")))
		})?;

		self.validate_ip(&ip)?;

		Ok(())
	}

	pub(crate) fn validate_ip(&self, ip: &IPAddress) -> Result<()> {
		if !self.services.client.valid_cidr_range(ip) {
			return Err!(BadServerResponse("Not allowed to send requests to this IP"));
		}

		Ok(())
	}
}
