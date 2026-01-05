use std::{fmt::Debug, mem};

use bytes::BytesMut;
use conduwuit::{Err, Result, config::MeowlnirConfig, debug_error, err, utils, warn};
use reqwest::Client;
use ruma::api::{IncomingResponse, MatrixVersion, OutgoingRequest, SendAccessToken};

/// Sends a request to an antispam service
pub(crate) async fn send_meowlnir_request<T>(
	client: &Client,
	config: &MeowlnirConfig,
	request: T,
) -> Result<Option<T::IncomingResponse>>
where
	T: OutgoingRequest + Debug + Send,
{
	const VERSIONS: [MatrixVersion; 1] = [MatrixVersion::V1_15];
	if config.secret.is_empty() {
		return Ok(None);
	}
	let secret = config.secret.as_str();
	let http_request = request
		.try_into_http_request::<BytesMut>(
			config.base_url.as_str(),
			SendAccessToken::Always(secret),
			&VERSIONS,
		)?
		.map(BytesMut::freeze);
	let reqwest_request = reqwest::Request::try_from(http_request)?;

	let mut response = client.execute(reqwest_request).await.map_err(|e| {
		warn!("Could not send request to antispam: {e:?}");
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

	let body = response.bytes().await?; // TODO: handle timeout

	if !status.is_success() {
		debug_error!("Antispam response bytes: {:?}", utils::string_from_bytes(&body));
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

	response.map(Some).map_err(|e| {
		err!(BadServerResponse(warn!(
			"Antispam returned invalid/malformed response bytes: {e}",
		)))
	})
}
