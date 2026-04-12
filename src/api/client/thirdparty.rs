use std::collections::BTreeMap;

use conduwuit::Result;
use ruma::api::client::thirdparty::get_protocols;

use crate::{Ruma, RumaResponse};

/// # `GET /_matrix/client/r0/thirdparty/protocols`
///
/// TODO: Fetches all metadata about protocols supported by the homeserver.
pub(crate) async fn get_protocols_route(
	_body: Ruma<get_protocols::v3::Request>,
) -> Result<get_protocols::v3::Response> {
	// We don't support this at all
	Ok(get_protocols::v3::Response::new(BTreeMap::new()))
}

/// # `GET /_matrix/client/unstable/thirdparty/protocols`
///
/// Same as `get_protocols_route`, except for some reason Element Android legacy
/// calls this
pub(crate) async fn get_protocols_route_unstable(
	body: Ruma<get_protocols::v3::Request>,
) -> Result<RumaResponse<get_protocols::v3::Response>> {
	get_protocols_route(body).await.map(RumaResponse)
}
