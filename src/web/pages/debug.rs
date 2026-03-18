use std::convert::Infallible;

use axum::{Router, routing::get};
use conduwuit_core::Error;

use crate::WebError;

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/_debug/panic", get(async || -> Infallible { panic!("Guru meditation error") }))
		.route(
			"/_debug/error",
			get(async || -> WebError {
				Error::Err(std::borrow::Cow::Borrowed("Guru meditation error")).into()
			}),
		)
}
