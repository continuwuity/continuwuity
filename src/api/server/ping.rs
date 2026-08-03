use std::{sync::Arc, time::Duration};

use axum::extract::State;
use conduwuit::{
	Result, info,
	utils::{random_string, time::jitter},
	warn,
};
use ruma::{OwnedServerName, api::error::ErrorKind};
use ruminuwuity::api::federation::{
	ping::unstable::{Request, Response},
	pong,
};

use crate::Ruma;

/// # `GET /_matrix/federation/unstable/uk.timedout.msc0000.tabletennis/ping`
///
/// Initiates a ping
pub(crate) async fn ping(
	State(services): State<crate::State>,
	body: Ruma<Request>,
) -> Result<Response> {
	// TODO: rate-limiting
	// TODO: make answer stable based on question
	let answer = random_string(64);
	services.federation.register_ping_answer(answer.clone())?;
	services
		.server
		.runtime()
		.spawn(send_ping(Arc::new(services), body.identity, answer.clone()));
	Ok(Response::new(answer))
}

async fn send_ping(services: Arc<crate::State>, target: OwnedServerName, answer: String) {
	tokio::time::sleep(jitter(Duration::from_secs(1), 1.0..=10.0)).await;
	info!(%answer, "Sending a federation pong to {target}");
	match services
		.sending
		.send_federation_request(&target, pong::unstable::Request::new(answer))
		.await
	{
		| Ok(_) => info!(%target, "Federation pong succeeded"),
		| Err(e) =>
			if e.is_not_found() {
				if e.kind() == ErrorKind::Unrecognized {
					warn!(%target, "Remote requested a ping but does not recognize the pong.");
				} else {
					info!(%target, "Ping expired by the time we ponged.");
				}
			} else {
				warn!(%target, "Pong failed");
			},
	}
}
