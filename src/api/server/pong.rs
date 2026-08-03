use axum::extract::State;
use conduwuit::Result;
use ruminuwuity::api::federation::pong;

use crate::Ruma;

/// # `GET /_matrix/federation/unstable/uk.timedout.msc0000.tabletennis/pong`
///
/// Responds to a ping
pub(crate) async fn pong(
	State(services): State<crate::State>,
	body: Ruma<pong::unstable::Request>,
) -> Result<pong::unstable::Response> {
	// TODO: verify incoming question is already registered
	services.federation.answer_ping(&body.question)?;
	Ok(pong::unstable::Response::default())
}
