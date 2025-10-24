use axum::extract::State;
use conduwuit::Result;
use ruma::api::federation::edutypes::get_edutypes;

use crate::Ruma;

/// # `GET /_matrix/federation/v1/edutypes`
///
/// Lists EDU types we wish to receive
pub(crate) async fn get_edutypes_route(
	State(services): State<crate::State>,
	_body: Ruma<get_edutypes::unstable::Request>,
) -> Result<get_edutypes::unstable::Response> {
	Ok(get_edutypes::unstable::Response {
		typing: services.config.allow_incoming_typing,
		presence: services.config.allow_incoming_presence,
		receipt: services.config.allow_incoming_read_receipts,
	})
}
