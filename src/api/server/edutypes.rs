use axum::extract::State;
use conduwuit::Result;
use ruma::{api::federation::query::get_edu_types, assign};

use crate::Ruma;

/// # `GET /_matrix/federation/v1/edutypes`
///
/// Lists EDU types we wish to receive
pub(crate) async fn get_edutypes_route(
	State(services): State<crate::State>,
	_body: Ruma<get_edu_types::unstable::Request>,
) -> Result<get_edu_types::unstable::Response> {
	Ok(assign!(get_edu_types::unstable::Response::new(), {
		typing: services.config.allow_incoming_typing,
		presence: services.config.allow_incoming_presence,
		receipt: services.config.allow_incoming_read_receipts,
	}))
}
