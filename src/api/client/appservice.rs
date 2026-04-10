use axum::extract::State;
use conduwuit::{Err, Result, err};
use ruma::{
	api::{appservice::ping, client::appservice::request_ping},
	assign,
};

use crate::Ruma;

/// # `POST /_matrix/client/v1/appservice/{appserviceId}/ping`
///
/// Ask the homeserver to ping the application service to ensure the connection
/// works.
pub(crate) async fn appservice_ping(
	State(services): State<crate::State>,
	body: Ruma<request_ping::v1::Request>,
) -> Result<request_ping::v1::Response> {
	let appservice_info = body.appservice_info.as_ref().ok_or_else(|| {
		err!(Request(Forbidden("This endpoint can only be called by appservices.")))
	})?;

	if body.appservice_id != appservice_info.registration.id {
		return Err!(Request(Forbidden(
			"Appservices can only ping themselves (wrong appservice ID)."
		)));
	}

	if appservice_info.registration.url.is_none()
		|| appservice_info
			.registration
			.url
			.as_ref()
			.is_some_and(|url| url.is_empty() || url == "null")
	{
		return Err!(Request(UrlNotSet(
			"Appservice does not have a URL set, there is nothing to ping."
		)));
	}

	let timer = tokio::time::Instant::now();

	let _response = services
		.sending
		.send_appservice_request(
			appservice_info.registration.clone(),
			assign!(ping::send_ping::v1::Request::new(), {
				transaction_id: body.transaction_id.clone(),
			}),
		)
		.await?
		.expect("We already validated if an appservice URL exists above");

	Ok(request_ping::v1::Response::new(timer.elapsed()))
}
