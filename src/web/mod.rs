use std::any::Any;

use askama::Template;
use axum::{
	Router,
	extract::rejection::{FormRejection, QueryRejection},
	http::{HeaderValue, StatusCode, header},
	response::{Html, IntoResponse, Response},
};
use conduwuit_service::state;
use tower_http::{catch_panic::CatchPanicLayer, set_header::SetResponseHeaderLayer};
use tower_sec_fetch::SecFetchLayer;

use crate::pages::TemplateContext;

mod pages;

type State = state::State;

const CATASTROPHIC_FAILURE: &str = "cat-astrophic failure! we couldn't even render the error template. \
please contact the team @ https://continuwuity.org";

#[derive(Debug, thiserror::Error)]
enum WebError {
	#[error("Failed to validate form body: {0}")]
	ValidationError(#[from] validator::ValidationErrors),
	#[error("{0}")]
	QueryRejection(#[from] QueryRejection),
	#[error("{0}")]
	FormRejection(#[from] FormRejection),
	#[error("{0}")]
	BadRequest(String),

	#[error("This page does not exist.")]
	NotFound,

	#[error("Failed to render template: {0}")]
	Render(#[from] askama::Error),
	#[error("{0}")]
	InternalError(#[from] conduwuit_core::Error),
	#[error("Request handler panicked! {0}")]
	Panic(String),
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
				// Statically set false to prevent error pages from being indexed.
				allow_indexing: false,
			},
		};

		if let Ok(body) = template.render() {
			(status, Html(body)).into_response()
		} else {
			(status, CATASTROPHIC_FAILURE).into_response()
		}
	}
}

pub fn build() -> Router<state::State> {
	#[allow(clippy::wildcard_imports)]
	use pages::*;

	Router::new()
		.merge(index::build())
		.nest(
			"/_continuwuity/",
			Router::new()
				.merge(resources::build())
				.merge(password_reset::build())
				.merge(debug::build())
				.merge(threepid::build())
				.fallback(async || WebError::NotFound),
		)
		.layer(CatchPanicLayer::custom(|panic: Box<dyn Any + Send + 'static>| {
			let details = if let Some(s) = panic.downcast_ref::<String>() {
				s.clone()
			} else if let Some(s) = panic.downcast_ref::<&str>() {
				(*s).to_owned()
			} else {
				"(opaque panic payload)".to_owned()
			};

			WebError::Panic(details).into_response()
		}))
		.layer(SetResponseHeaderLayer::if_not_present(
			header::CONTENT_SECURITY_POLICY,
			HeaderValue::from_static("default-src 'self'; img-src 'self' data:;"),
		))
		.layer(SecFetchLayer::new(|policy| {
			policy.allow_safe_methods().reject_missing_metadata();
		}))
}
