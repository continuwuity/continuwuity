use axum::{response::Response, routing::MethodFilter};

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

#[derive(Debug)]
pub(crate) struct TemplateContext {
	pub allow_indexing: bool,
}

impl From<&crate::State> for TemplateContext {
	fn from(state: &crate::State) -> Self {
		Self {
			allow_indexing: state.config.allow_web_indexing,
		}
	}
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
            fn new(state: &$crate::State, $($field_name: $field_type,)*) -> Self {
                Self {
                    context: state.into(),
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
