use std::ops::Deref;

use axum::{
	RequestExt, RequestPartsExt,
	body::Body,
	extract::{FromRequest, Path, Query},
};
use conduwuit::{Error, Result, err};
use ruma::{CanonicalJsonObject, api::IncomingRequest};
use serde::Deserialize;

use crate::{State, router::auth::CheckAuth};

/// Query parameters needed to authenticate requests
#[derive(Deserialize)]
pub(crate) struct AuthQueryParams {
	pub(super) user_id: Option<String>,
	pub(super) device_id: Option<String>,
	#[serde(rename = "org.matrix.msc3202.device_id")]
	pub(super) legacy_device_id: Option<String>,
}

/// Extractor for Ruma request structs
pub(crate) struct Args<R: IncomingRequest<Authentication: CheckAuth> + Send + Sync + 'static> {
	/// Request struct body
	pub(crate) body: R,

	/// Parsed JSON body. None when body is not JSON.
	pub(crate) json_body: Option<CanonicalJsonObject>,

	/// Identity of the requesting entity
	pub(crate) identity: <R::Authentication as CheckAuth>::Identity,
}

impl<R> Deref for Args<R>
where
	R: IncomingRequest<Authentication: CheckAuth> + Send + Sync + 'static,
{
	type Target = R;

	fn deref(&self) -> &Self::Target { &self.body }
}

impl<R> FromRequest<State, Body> for Args<R>
where
	R: IncomingRequest<Authentication: CheckAuth> + Send + Sync + 'static,
{
	type Rejection = Error;

	async fn from_request(
		request: hyper::Request<Body>,
		services: &State,
	) -> Result<Self, Self::Rejection> {
		let limited = request.with_limited_body();

		let (mut parts, body) = limited.into_parts();

		// Read the body
		let body = {
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

			axum::body::to_bytes(body, max_body_size)
				.await
				.map_err(|e| err!(Request(TooLarge("Request body too large: {e}"))))?
		};

		// Make a JSON copy of the body for use in handlers
		let json_body = serde_json::from_slice::<CanonicalJsonObject>(&body).ok();

		// Extract the query parameters and path
		let Path(path): Path<Vec<String>> = parts.extract().await?;
		let Query(auth_query): Query<AuthQueryParams> = parts.extract().await?;

		// Assemble a new request from the read body and parts
		let request = hyper::Request::from_parts(parts, body);

		// Check authentication
		let auth =
			R::Authentication::authenticate::<R, bytes::Bytes>(services, &request, auth_query)
				.await?;

		// Deserialize the body
		let body = R::try_from_http_request(request, &path)
			.map_err(|e| err!(Request(BadJson(debug_warn!("{e}")))))?;

		Ok(Self { body, json_body, identity: auth })
	}
}
