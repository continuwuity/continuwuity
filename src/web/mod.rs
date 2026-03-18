use askama::Template;
use axum::{
	Router,
	extract::rejection::{FormRejection, QueryRejection},
	http::{HeaderValue, StatusCode, header},
	response::{Html, IntoResponse, Response},
};
use conduwuit_service::state;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_sec_fetch::SecFetchLayer;

use crate::pages::TemplateContext;

mod pages;

type State = state::State;

#[derive(Debug, thiserror::Error)]
enum WebError {
	#[error("Failed to render template: {0}")]
	Render(#[from] askama::Error),
	#[error("Failed to validate form body: {0}")]
	ValidationError(#[from] validator::ValidationErrors),

	#[error("{0}")]
	QueryRejection(#[from] QueryRejection),
	#[error("{0}")]
	FormRejection(#[from] FormRejection),

	#[error("Bad request: {0}")]
	BadRequest(String),
	#[error("This page does not exist.")]
	NotFound,
	#[error("Internal server error: {0}")]
	InternalError(#[from] conduwuit_core::Error),
}

impl IntoResponse for WebError {
	fn into_response(self) -> Response {
		#[derive(Debug, Template)]
		#[template(path = "error.html.j2")]
		struct Error {
			error: WebError,
			status: StatusCode,
			context: TemplateContext,
		}

		let status = match &self {
			| Self::ValidationError(_)
			| Self::BadRequest(_)
			| Self::QueryRejection(_)
			| Self::FormRejection(_) => StatusCode::BAD_REQUEST,
			| Self::NotFound => StatusCode::NOT_FOUND,
			| _ => StatusCode::INTERNAL_SERVER_ERROR,
		};

		let template = Error {
			error: self,
			status,
			context: TemplateContext {
				// Statically set false to prevent error pages from being indexed and to prevent
				// further errors if services.config is having issues.
				allow_indexing: false,
			},
		};

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

	let sub_router = Router::new()
		.merge(resources::build())
		.merge(password_reset::build())
		.fallback(async || WebError::NotFound);

	Router::new()
		.merge(index::build())
		.nest("/_continuwuity/", sub_router)
		.layer(SetResponseHeaderLayer::if_not_present(
			header::CONTENT_SECURITY_POLICY,
			HeaderValue::from_static("default-src 'self'; img-src 'self' data:;"),
		))
		.layer(SecFetchLayer::new(|policy| {
			policy.allow_safe_methods().reject_missing_metadata();
		}))
}
