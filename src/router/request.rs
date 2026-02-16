use std::{
	fmt::Debug,
	sync::{Arc, atomic::Ordering},
	time::Duration,
};

use axum::{
	extract::State,
	response::{IntoResponse, Response},
};
use conduwuit::{Result, debug_warn, err, error, trace, info};
use conduwuit_service::Services;
use futures::FutureExt;
use http::{Method, StatusCode, Uri};
use tokio::time::sleep;
use tracing::Span;

#[tracing::instrument(name = "request", level = "debug", skip_all)]
pub(crate) async fn handle(
	State(services): State<Arc<Services>>,
	req: http::Request<axum::body::Body>,
	next: axum::middleware::Next,
) -> Result<Response, StatusCode> {
	if !services.server.running() {
		debug_warn!(
			method = %req.method(),
			uri = %req.uri(),
			"unavailable pending shutdown"
		);

		return Err(StatusCode::SERVICE_UNAVAILABLE);
	}

	let uri = req.uri().clone();
	let method = req.method().clone();
	let services_ = services.clone();
	let parent = Span::current();
	let task = services.server.runtime().spawn(async move {
		tokio::select! {
			response = execute(&services_, req, next, &parent) => response,
			response = services_.server.until_shutdown()
				.then(|()| {
					let timeout = services_.server.config.client_shutdown_timeout;
					let timeout = Duration::from_secs(timeout);
					sleep(timeout)
				})
				.map(|()| StatusCode::SERVICE_UNAVAILABLE)
				.map(IntoResponse::into_response) => response,
		}
	});

	task.await
		.map_err(unhandled)
		.and_then(move |result| handle_result(&method, &uri, result))
}

#[tracing::instrument(
	name = "handle",
	level = "debug",
	parent = parent,
	skip_all,
	fields(
		active = %services
			.server
			.metrics
			.requests_handle_active
			.fetch_add(1, Ordering::Relaxed),
		handled = %services
			.server
			.metrics
			.requests_handle_finished
			.load(Ordering::Relaxed),
	)
)]
async fn execute(
	// we made a safety contract that Services will not go out of scope
	// during the request; this ensures a reference is accounted for at
	// the base frame of the task regardless of its detachment.
	services: &Arc<Services>,
	req: http::Request<axum::body::Body>,
	next: axum::middleware::Next,
	parent: &Span,
) -> Response {
	#[cfg(debug_assertions)]
	conduwuit::defer! {{
		_ = services.server
			.metrics
			.requests_handle_finished
			.fetch_add(1, Ordering::Relaxed);
		_ = services.server
			.metrics
			.requests_handle_active
			.fetch_sub(1, Ordering::Relaxed);
	}};

	next.run(req).await
}

fn handle_result(method: &Method, uri: &Uri, result: Response) -> Result<Response, StatusCode> {
	let status = result.status();
	let code = status.as_u16();
	let reason = status.canonical_reason().unwrap_or("Unknown Reason");

	if status.is_server_error() {
		info!(%method, %uri, "{code} {reason}");
	} else if status.is_client_error() {
		info!(%method, %uri, "{code} {reason}");
	} else if status.is_redirection() {
		trace!(%method, %uri, "{code} {reason}");
	} else {
		trace!(%method, %uri, "{code} {reason}");
	}

	if status == StatusCode::METHOD_NOT_ALLOWED {
		return Ok(err!(Request(Unrecognized("Method Not Allowed"))).into_response());
	}

	Ok(result)
}

#[cold]
fn unhandled<Error: Debug>(e: Error) -> StatusCode {
	error!("unhandled error or panic during request: {e:?}");

	StatusCode::INTERNAL_SERVER_ERROR
}
