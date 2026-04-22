use std::ops::Deref;

use axum::{
	RequestExt, RequestPartsExt,
	body::Body,
	extract::{FromRequest, Path, Query},
};
use conduwuit::{Error, Result, err};
use ruma::{
	CanonicalJsonObject, DeviceId, OwnedDeviceId, OwnedServerName, OwnedUserId, ServerName,
	UserId, api::IncomingRequest,
};
use serde::Deserialize;

use crate::{State, router::auth::CheckAuth, service::appservice::RegistrationInfo};

/// Query parameters needed to authenticate requests
#[derive(Deserialize)]
pub(super) struct AuthQueryParams {
	pub(super) user_id: Option<String>,
	/// Device ID for appservice device masquerading (MSC3202/MSC4190).
	/// Can be provided as `device_id` or `org.matrix.msc3202.device_id`.
	#[serde(alias = "org.matrix.msc3202.device_id")]
	pub(super) device_id: Option<String>,
}

/// Extractor for Ruma request structs
pub(crate) struct Args<T> {
	/// Request struct body
	pub(crate) body: T,

	/// Federation server authentication: X-Matrix origin
	/// None when not a federation server.
	pub(crate) origin: Option<OwnedServerName>,

	/// Local user authentication: user_id.
	/// None when not an authenticated local user.
	pub(crate) sender_user: Option<OwnedUserId>,

	/// Local user authentication: device_id.
	/// None when not an authenticated local user or no device.
	pub(crate) sender_device: Option<OwnedDeviceId>,

	/// Appservice authentication; registration info.
	/// None when not an appservice.
	pub(crate) appservice_info: Option<RegistrationInfo>,

	/// Parsed JSON content.
	/// None when body is not a valid string
	pub(crate) json_body: Option<CanonicalJsonObject>,
}

impl<T> Args<T>
where
	T: IncomingRequest + Send + Sync + 'static,
{
	#[inline]
	pub(crate) fn sender(&self) -> (&UserId, &DeviceId) {
		(self.sender_user(), self.sender_device())
	}

	#[inline]
	pub(crate) fn sender_user(&self) -> &UserId {
		self.sender_user
			.as_deref()
			.expect("user must be authenticated for this handler")
	}

	#[inline]
	pub(crate) fn sender_device(&self) -> &DeviceId {
		self.sender_device
			.as_deref()
			.expect("user must be authenticated and device identified")
	}

	#[inline]
	pub(crate) fn origin(&self) -> &ServerName {
		self.origin
			.as_deref()
			.expect("server must be authenticated for this handler")
	}
}

impl<T> Deref for Args<T>
where
	T: IncomingRequest + Send + Sync + 'static,
{
	type Target = T;

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

		Ok(Self {
			body,
			origin: auth.origin,
			sender_user: auth.sender_user,
			sender_device: auth.sender_device,
			appservice_info: auth.appservice_info,
			json_body,
		})
	}
}
