#[cfg(unix)]
use std::path::Path;
use std::sync::Arc;

use axum::{Router, extract::State, response::IntoResponse, routing::get};
use conduwuit::{
	Result, err, error, info, metrics::PROMETHEUS_CONTENT_TYPE, utils::string::constant_time_eq,
	warn,
};
use conduwuit_service::Services;
use http::{HeaderMap, StatusCode, header};
#[cfg(unix)]
use tokio::{fs, net::UnixListener};
use tokio::{net::TcpListener, sync::broadcast};

pub(super) async fn serve(
	services: &Arc<Services>,
	mut shutdown: broadcast::Receiver<()>,
) -> Result {
	let config = &services.server.config;
	if !config.allow_prometheus_metrics {
		return Ok(());
	}

	let app = Router::new()
		.route("/metrics", get(scrape))
		.with_state(services.clone());
	let shutdown = async move { _ = shutdown.recv().await };

	#[cfg(unix)]
	if let Some(path) = &config.prometheus_metrics_unix_socket_path {
		let listener = bind_unix(path, config.unix_socket_perms).await?;
		info!("Serving metrics at {path:?}");
		let result = axum::serve(listener, app)
			.with_graceful_shutdown(shutdown)
			.await;

		if let Err(e) = fs::remove_file(path).await {
			warn!(?path, "Failed to remove UNIX socket file: {e}");
		}

		return result.map_err(|e| err!(error!("metrics listener failed: {e}")));
	}

	let addr = config.prometheus_metrics_address;
	let listener = TcpListener::bind(addr)
		.await
		.map_err(|e| err!(Config("prometheus_metrics_address", "{e}")))?;
	info!("Serving metrics on {addr}");

	axum::serve(listener, app)
		.with_graceful_shutdown(shutdown)
		.await
		.map_err(|e| err!(error!("metrics listener failed: {e}")))
}

#[cfg(unix)]
async fn bind_unix(path: &Path, perms: u32) -> Result<UnixListener> {
	use std::os::unix::fs::PermissionsExt;

	if path.exists() {
		warn!("Removing existing UNIX socket {path:?} (unclean shutdown?)...");
		fs::remove_file(path)
			.await
			.map_err(|e| err!("Failed to remove existing UNIX socket {path:?}: {e}"))?;
	}

	let dir = path.parent().unwrap_or_else(|| Path::new("/"));
	fs::create_dir_all(dir)
		.await
		.map_err(|e| err!("Failed to create {dir:?} for socket {path:?}: {e}"))?;

	let listener =
		UnixListener::bind(path).map_err(|e| err!("Failed to bind listener {path:?}: {e}"))?;

	let mode = u32::from_str_radix(&perms.to_string(), 8)
		.map_err(|e| err!("Invalid unix_socket_perms {perms}: {e}"))?;
	fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
		.await
		.map_err(|e| err!("Failed to set socket {path:?} permissions: {e}"))?;

	Ok(listener)
}

async fn scrape(
	State(services): State<Arc<Services>>,
	headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
	if let Some(token) = &services.globals.prometheus_metrics_token {
		let authorized = headers
			.get(header::AUTHORIZATION)
			.and_then(|value| value.to_str().ok())
			.and_then(|value| value.split_once(' '))
			.is_some_and(|(scheme, provided)| {
				scheme.eq_ignore_ascii_case("bearer") && constant_time_eq(provided, token)
			});

		if !authorized {
			return Err(StatusCode::UNAUTHORIZED);
		}
	}

	let body = services.server.metrics.render().map_err(|e| {
		error!("Failed to render metrics: {e}");
		StatusCode::INTERNAL_SERVER_ERROR
	})?;

	Ok(([(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)], body))
}
