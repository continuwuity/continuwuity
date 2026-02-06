use askama::Template;
use axum::{
	Router,
	extract::State,
	http::{StatusCode, header},
	response::{Html, IntoResponse, Response},
	routing::get,
};
use conduwuit_build_metadata::{GIT_REMOTE_COMMIT_URL, GIT_REMOTE_WEB_URL, version_tag};
use conduwuit_service::state;
use snafu::{IntoError, prelude::*};

pub fn build() -> Router<state::State> {
	let router = Router::<state::State>::new();
	router.route("/", get(index_handler))
}

async fn index_handler(
	State(services): State<state::State>,
) -> Result<impl IntoResponse, WebError> {
	#[derive(Debug, Template)]
	#[template(path = "index.html.j2")]
	struct Tmpl<'a> {
		nonce: &'a str,
		server_name: &'a str,
	}
	let nonce = rand::random::<u64>().to_string();

	let template = Tmpl {
		nonce: &nonce,
		server_name: services.config.server_name.as_str(),
	};
	Ok((
		[(header::CONTENT_SECURITY_POLICY, format!("default-src 'none' 'nonce-{nonce}';"))],
		Html(template.render()?),
	))
}

#[derive(Debug, Snafu)]
enum WebError {
	#[snafu(display("Failed to render template: {source}"))]
	Render {
		source: askama::Error,
		backtrace: snafu::Backtrace,
	},
}

impl From<askama::Error> for WebError {
	fn from(source: askama::Error) -> Self { RenderSnafu.into_error(source) }
}

impl IntoResponse for WebError {
	fn into_response(self) -> Response {
		#[derive(Debug, Template)]
		#[template(path = "error.html.j2")]
		struct Tmpl<'a> {
			nonce: &'a str,
			err: WebError,
		}

		let nonce = rand::random::<u64>().to_string();

		let status = match &self {
			| Self::Render { .. } => StatusCode::INTERNAL_SERVER_ERROR,
		};
		let tmpl = Tmpl { nonce: &nonce, err: self };
		if let Ok(body) = tmpl.render() {
			(
				status,
				[(
					header::CONTENT_SECURITY_POLICY,
					format!("default-src 'none' 'nonce-{nonce}';"),
				)],
				Html(body),
			)
				.into_response()
		} else {
			(status, "Something went wrong").into_response()
		}
	}
}
