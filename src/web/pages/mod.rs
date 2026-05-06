use std::sync::Arc;

use axum::{
	extract::{Request, State},
	http::{HeaderValue, header},
	middleware::Next,
	response::Response,
	routing::MethodFilter,
};
use conduwuit_core::utils;

use crate::WebError;

pub(super) mod account;
mod components;
pub(super) mod debug;
pub(super) mod index;
pub(super) mod oauth;
pub(super) mod resources;
pub(super) mod threepid;

type Result<T = Response, E = WebError> = std::result::Result<T, E>;

const GET_POST: MethodFilter = MethodFilter::GET.or(MethodFilter::POST);

#[derive(Debug, Clone)]
pub(crate) struct TemplateContext {
	pub allow_indexing: bool,
	pub csp_nonce: String,
}

const CSP_NONCE_LENGTH: usize = 32;

pub(super) async fn template_context_middleware(
	State(config): State<Arc<conduwuit_service::config::Service>>,
	mut request: Request,
	next: Next,
) -> Response {
	let csp_nonce = utils::random_string(CSP_NONCE_LENGTH);
	let context = TemplateContext {
		allow_indexing: config.allow_web_indexing,
		csp_nonce: csp_nonce.clone(),
	};

	assert!(
		request.extensions_mut().insert(context).is_none(),
		"template context should only be inserted once"
	);

	let mut response = next.run(request).await;

	let child_src = if config.recaptcha_site_key.is_some() {
		"www.google.com"
	} else {
		"'none'"
	};

	response.headers_mut().insert(
		header::CONTENT_SECURITY_POLICY,
		HeaderValue::from_str(&format!(
			"default-src 'none'; style-src 'self'; img-src 'self' 'https' data:; script-src \
			 'nonce-{csp_nonce}'; child-src {child_src};"
		))
		.expect("should be able to build CSP header"),
	);

	response
}

#[macro_export]
macro_rules! template {
    (
        struct $name:ident $(<$lifetime:lifetime>)? use $path:literal {
            $($field_name:ident: $field_type:ty),*
        }
    ) => {
        #[derive(Debug, askama::Template)]
        #[template(path = $path)]
        struct $name$(<$lifetime>)? {
            context: $crate::pages::TemplateContext,
            $($field_name: $field_type,)*
        }

        impl$(<$lifetime>)? $name$(<$lifetime>)? {
            #[allow(clippy::too_many_arguments)]
            fn new(context: $crate::pages::TemplateContext, $($field_name: $field_type,)*) -> Self {
                Self {
                    context,
                    $($field_name,)*
                }
            }
        }

        #[allow(single_use_lifetimes)]
        impl$(<$lifetime>)? axum::response::IntoResponse for $name$(<$lifetime>)? {
            fn into_response(self) -> axum::response::Response {
                use askama::Template;

                match self.render() {
                    Ok(rendered) => axum::response::Html(rendered).into_response(),
                    Err(err) => $crate::WebError::from(err).into_response()
                }
            }
        }
    };
}

#[macro_export]
macro_rules! response {
	(BadRequest($body:expr)) => {
		response!((axum::http::StatusCode::BAD_REQUEST, $body))
	};

	($body:expr) => {{
		use axum::response::IntoResponse;

		Ok($body.into_response())
	}};
}
