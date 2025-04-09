use std::{net::SocketAddr, sync::Arc};

use axum::Router;
use axum_server::Handle as ServerHandle;
use axum_server_dual_protocol::{
	ServerExt,
	axum_server::{bind_rustls, tls_rustls::RustlsConfig},
};
use conduwuit::{Result, Server, err};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

pub(super) async fn serve(
	server: &Arc<Server>,
	app: Router,
	handle: ServerHandle,
	addrs: Vec<SocketAddr>,
) -> Result {
	let tls = &server.config.tls;
	let certs = tls.certs.as_ref().ok_or_else(|| {
		err!(Config("tls.certs", "Missing required value in tls config section"))
	})?;
	let key = tls
		.key
		.as_ref()
		.ok_or_else(|| err!(Config("tls.key", "Missing required value in tls config section")))?;

	// we use ring for ruma and hashing state, but aws-lc-rs is the new default.
	// without this, TLS mode will panic.
	rustls::crypto::aws_lc_rs::default_provider()
		.install_default()
		.expect("failed to initialise aws-lc-rs rustls crypto provider");

	info!(
		"Note: It is strongly recommended that you use a reverse proxy instead of running \
		 conduwuit directly with TLS."
	);
	debug!("Using direct TLS. Certificate path {certs} and certificate private key path {key}",);
	let conf = RustlsConfig::from_pem_file(certs, key)
		.await
		.map_err(|e| err!(Config("tls", "Failed to load certificates or key: {e}")))?;

	let mut join_set = JoinSet::new();
	let app = app.into_make_service_with_connect_info::<SocketAddr>();
	if tls.dual_protocol {
		for addr in &addrs {
			join_set.spawn_on(
				axum_server_dual_protocol::bind_dual_protocol(*addr, conf.clone())
					.set_upgrade(false)
					.handle(handle.clone())
					.serve(app.clone()),
				server.runtime(),
			);
		}
	} else {
		for addr in &addrs {
			join_set.spawn_on(
				bind_rustls(*addr, conf.clone())
					.handle(handle.clone())
					.serve(app.clone()),
				server.runtime(),
			);
		}
	}

	if tls.dual_protocol {
		warn!(
			"Listening on {addrs:?} with TLS certificate {certs} and supporting plain text \
			 (HTTP) connections too (insecure!)",
		);
	} else {
		info!("Listening on {addrs:?} with TLS certificate {certs}");
	}

	while join_set.join_next().await.is_some() {}

	Ok(())
}
