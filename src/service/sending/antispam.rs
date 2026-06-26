use std::{borrow::Cow, fmt::Debug, mem, time::Instant};

use bytes::BytesMut;
use conduwuit::{Err, Result, debug_info, err, utils::response::LimitReadExt};
use reqwest::Client;
use ruma::api::{
	IncomingResponse, OutgoingRequest,
	auth_scheme::{AppserviceToken, SendAccessToken},
	path_builder::VersionHistory,
};

use crate::SUPPORTED_VERSIONS;

/// Sends a request to an antispam service
pub(crate) async fn send_antispam_request<T>(
	client: &Client,
	base_url: &str,
	secret: &str,
	request: T,
) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest<Authentication = AppserviceToken, PathBuilder = VersionHistory>
		+ Debug
		+ Send,
{
	let http_request = request
		.try_into_http_request::<BytesMut>(
			base_url,
			SendAccessToken::Always(secret),
			Cow::Borrowed(&SUPPORTED_VERSIONS),
		)?
		.map(BytesMut::freeze);
	let reqwest_request = reqwest::Request::try_from(http_request)?;

	let method = reqwest_request.method().clone();
	let url = reqwest_request.url().clone();
	debug_info!("Sending request to appservice: {} {}", method, url);
	let start = Instant::now();
	let mut response = client.execute(reqwest_request).await.map_err(|e| {
		err!(BadServerResponse(error!(?e, "Failed to contact antispam service.")))
	})?;
	debug_info!(
		"Received response (HTTP {}) from antispam service in {:?}: {} {}",
		response.status(),
		start.elapsed().as_millis(),
		method,
		url,
	);

	// reqwest::Response -> http::Response conversion
	let status = response.status();
	let mut http_response_builder = http::Response::builder()
		.status(status)
		.version(response.version());
	mem::swap(
		response.headers_mut(),
		http_response_builder
			.headers_mut()
			.expect("http::response::Builder is usable"),
	);

	let body = response.limit_read(65535).await.map_err(|e| {
		err!(BadServerResponse(error!(
			?e,
			"Failed to read response body from antispam service."
		)))
	})?; // TODO: handle timeout

	if !status.is_success() {
		return match status {
			| http::StatusCode::FORBIDDEN =>
				Err!(Request(Forbidden("Request was rejected by antispam service.",))),
			| _ => Err!(BadServerResponse(warn!(
				"Antispam returned unsuccessful HTTP response {status}",
			))),
		};
	}

	let response = T::IncomingResponse::try_from_http_response(
		http_response_builder
			.body(body)
			.expect("reqwest body is valid http body"),
	);

	response.map_err(|e| {
		err!(BadServerResponse(warn!(
			"Antispam returned invalid/malformed response bytes: {e}",
		)))
	})
}
