use std::{borrow::Cow, sync::Arc};

use axum::{Router, response::IntoResponse};
use conduwuit::Error;
use conduwuit_service::{Services, state, state::Guard};
use http::{StatusCode, Uri};
use ruma::api::client::error::ErrorKind;

pub(crate) fn build(services: &Arc<Services>) -> (Router, Guard) {
	let router = Router::<state::State>::new();
	let (state, guard) = state::create(services.clone());
	let router = conduwuit_api::router::build(router, &services.server)
		.merge(conduwuit_web::build())
		.fallback(not_found)
		.with_state(state);

	(router, guard)
}

async fn not_found(_uri: Uri) -> impl IntoResponse {
	Error::Request {
		kind: ErrorKind::Unrecognized,
		message: Cow::Borrowed("Not Found"),
		code: StatusCode::NOT_FOUND,
		backtrace: None,
	}
}
