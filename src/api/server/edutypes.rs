use conduwuit::{Config, Result};
use ruma::{api::federation::edutypes::get_edutypes};

use crate::Ruma;

/// # `GET /_matrix/federation/v1/edutypes`
///
/// Lists EDU types we wish to receive
pub(crate) async fn get_edutypes_route(
	body: Ruma<get_edutypes::v1::Request>,
	config: &Config,
) -> Result<get_edutypes::v1::Response> {
	Ok(get_edutypes::v1::Response {
		typing: config.allow_incoming_typing,
		presence: config.allow_incoming_presence,
		receipt: config.allow_incoming_read_receipts,
	})
}
