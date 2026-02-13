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

pub fn build() -> Router<state::State> {
	Router::<state::State>::new()
		.route("/", get(index_handler))
		.route("/_continuwuity/logo.svg", get(logo_handler))
}

async fn index_handler(
	State(services): State<state::State>,
) -> Result<impl IntoResponse, WebError> {
	#[derive(Debug, Template)]
	#[template(path = "index.html.j2")]
	struct Index<'a> {
		nonce: &'a str,
		server_name: &'a str,
		first_run: bool,
	}
	let nonce = rand::random::<u64>().to_string();

	let template = Index {
		nonce: &nonce,
		server_name: services.config.server_name.as_str(),
		first_run: services.firstrun.is_first_run(),
	};
	Ok((
		[(
			header::CONTENT_SECURITY_POLICY,
			format!("default-src 'nonce-{nonce}'; img-src 'self';"),
		)],
		Html(template.render()?),
	))
}

async fn logo_handler() -> impl IntoResponse {
	(
		[(header::CONTENT_TYPE, "image/svg+xml")],
		include_str!("templates/logo.svg").to_owned(),
	)
}

#[derive(Debug, thiserror::Error)]
enum WebError {
	#[error("Failed to render template: {0}")]
	Render(#[from] askama::Error),
}

impl IntoResponse for WebError {
	fn into_response(self) -> Response {
		#[derive(Debug, Template)]
		#[template(path = "error.html.j2")]
		struct Error<'a> {
			nonce: &'a str,
			err: WebError,
		}

		let nonce = rand::random::<u64>().to_string();

		let status = match &self {
			| Self::Render(_) => StatusCode::INTERNAL_SERVER_ERROR,
		};
		let tmpl = Error { nonce: &nonce, err: self };
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
