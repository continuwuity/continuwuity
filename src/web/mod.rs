use askama::Template;
use axum::{
	Router,
	http::{HeaderValue, StatusCode, header},
	response::{Html, IntoResponse, Response},
};
use conduwuit_service::state;
use tower_http::set_header::SetResponseHeaderLayer;

mod pages;

type State = state::State;

#[derive(Debug, thiserror::Error)]
enum WebError {
	#[error("Failed to render template: {0}")]
	Render(#[from] askama::Error),
}

impl IntoResponse for WebError {
	fn into_response(self) -> Response {
		#[derive(Debug, Template)]
		#[template(path = "error.html.j2")]
		struct Error {
			err: WebError,
		}

		let status = match &self {
			| Self::Render(_) => StatusCode::INTERNAL_SERVER_ERROR,
		};

		let tmpl = Error { err: self };

		if let Ok(body) = tmpl.render() {
			(status, Html(body)).into_response()
		} else {
			(status, "Something went wrong").into_response()
		}
	}
}

pub fn build() -> Router<state::State> {
	#[allow(clippy::wildcard_imports)]
	use pages::*;

	Router::new()
		.merge(index::build())
		.merge(resources::build())
		.layer(SetResponseHeaderLayer::if_not_present(
			header::CONTENT_SECURITY_POLICY,
			HeaderValue::from_static("default-src 'self'"),
		))
}
