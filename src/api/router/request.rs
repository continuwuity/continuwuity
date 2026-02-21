use std::str;

use axum::{RequestExt, RequestPartsExt, extract::Path};
use bytes::Bytes;
use conduwuit::{Result, err};
use http::request::Parts;
use serde::Deserialize;
use service::Services;

#[derive(Deserialize)]
pub(super) struct QueryParams {
	pub(super) access_token: Option<String>,
	pub(super) user_id: Option<String>,
	/// Device ID for appservice device masquerading (MSC3202/MSC4190).
	/// Can be provided as `device_id` or `org.matrix.msc3202.device_id`.
	#[serde(alias = "org.matrix.msc3202.device_id")]
	pub(super) device_id: Option<String>,
}

pub(super) struct Request {
	pub(super) path: Path<Vec<String>>,
	pub(super) query: QueryParams,
	pub(super) body: Bytes,
	pub(super) parts: Parts,
}

pub(super) async fn from(
	services: &Services,
	request: hyper::Request<axum::body::Body>,
) -> Result<Request> {
	let limited = request.with_limited_body();
	let (mut parts, body) = limited.into_parts();

	let path: Path<Vec<String>> = parts.extract().await?;
	let query = parts.uri.query().unwrap_or_default();
	let query = serde_html_form::from_str(query)
		.map_err(|e| err!(Request(Unknown("Failed to read query parameters: {e}"))))?;

	let max_body_size = services.server.config.max_request_size;

	// Check if the Content-Length header is present and valid, saves us streaming
	// the response into memory
	if let Some(content_length) = parts.headers.get(http::header::CONTENT_LENGTH) {
		if let Ok(content_length) = content_length
			.to_str()
			.map(|s| s.parse::<usize>().unwrap_or_default())
		{
			if content_length > max_body_size {
				return Err(err!(Request(TooLarge("Request body too large"))));
			}
		}
	}

	let body = axum::body::to_bytes(body, max_body_size)
		.await
		.map_err(|e| err!(Request(TooLarge("Request body too large: {e}"))))?;

	Ok(Request { path, query, body, parts })
}
