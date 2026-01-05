use std::{fmt::Debug, mem};

use bytes::BytesMut;
use conduwuit::{Err, Result, debug_error, err, implement, trace, utils, warn};
use ruma::api::{
	IncomingResponse, MatrixVersion, OutgoingRequest, SendAccessToken, appservice::Registration,
};

/// Sends a request to an appservice
///
/// Only returns Ok(None) if there is no url specified in the appservice
/// registration file
#[implement(super::Service)]
pub async fn send_appservice_request<T>(
	&self,
	registration: Registration,
	request: T,
) -> Result<Option<T::IncomingResponse>>
where
	T: OutgoingRequest + Debug + Send,
{
	const VERSIONS: [MatrixVersion; 1] = [MatrixVersion::V1_7];

	let Some(dest) = registration.url else {
		return Ok(None);
	};

	if dest == *"null" || dest.is_empty() {
		return Ok(None);
	}

	trace!("Appservice URL \"{dest}\", Appservice ID: {}", registration.id);

	let hs_token = registration.hs_token.as_str();
	let mut http_request = request
		.try_into_http_request::<BytesMut>(
			&dest,
			SendAccessToken::Appservice(hs_token),
			&VERSIONS,
		)
		.map_err(|e| {
			err!(BadServerResponse(
				warn!(appservice = %registration.id, "Failed to find destination {dest}: {e:?}")
			))
		})?
		.map(BytesMut::freeze);

	let mut parts = http_request.uri().clone().into_parts();
	let old_path_and_query = parts.path_and_query.unwrap().as_str().to_owned();
	let symbol = if old_path_and_query.contains('?') { "&" } else { "?" };

	parts.path_and_query = Some(
		(old_path_and_query + symbol + "access_token=" + hs_token)
			.parse()
			.unwrap(),
	);
	*http_request.uri_mut() = parts.try_into().expect("our manipulation is always valid");

	let reqwest_request = reqwest::Request::try_from(http_request)?;

	let client = &self.services.client.appservice;

	let mut response = client.execute(reqwest_request).await.map_err(|e| {
		warn!("Could not send request to appservice \"{}\" at {dest}: {e:?}", registration.id);
		e
	})?;

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

	let body = response.bytes().await?;

	if !status.is_success() {
		debug_error!("Appservice response bytes: {:?}", utils::string_from_bytes(&body));
		return Err!(BadServerResponse(warn!(
			"Appservice \"{}\" returned unsuccessful HTTP response {status} at {dest}",
			registration.id
		)));
	}

	let response = T::IncomingResponse::try_from_http_response(
		http_response_builder
			.body(body)
			.expect("reqwest body is valid http body"),
	);

	response.map(Some).map_err(|e| {
		err!(BadServerResponse(warn!(
			"Appservice \"{}\" returned invalid/malformed response bytes {dest}: {e}",
			registration.id
		)))
	})
}
