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
	#[error("Failed to validate form body: {0}")]
	ValidationError(#[from] validator::ValidationErrors),
	#[error("Bad request: {0}")]
	BadRequest(String),
	#[error("Internal server error: {0}")]
	InternalError(#[from] conduwuit_core::Error),
}

impl IntoResponse for WebError {
	fn into_response(self) -> Response {
		#[derive(Debug, Template)]
		#[template(path = "error.html.j2")]
		#[allow(unused)]
		struct Error {
			error: WebError,
			status: StatusCode,
		}

		let status = match &self {
			| Self::ValidationError(_) | Self::BadRequest(_) => StatusCode::BAD_REQUEST,
			| _ => StatusCode::INTERNAL_SERVER_ERROR,
		};

		let template = Error { error: self, status };

		if let Ok(body) = template.render() {
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
		.merge(password_reset::build())
		.layer(SetResponseHeaderLayer::if_not_present(
			header::CONTENT_SECURITY_POLICY,
			HeaderValue::from_static("default-src 'self'; img-src 'self' data:;"),
		))
}
