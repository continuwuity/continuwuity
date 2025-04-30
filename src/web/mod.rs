use askama::Template;
use axum::{
	Router,
	http::{StatusCode, header},
	response::{Html, IntoResponse, Response},
	routing::get,
};
use conduwuit_build_metadata::{GIT_REMOTE_COMMIT_URL, GIT_REMOTE_WEB_URL, VERSION_EXTRA};

pub fn build<S>() -> Router<()> { Router::new().route("/", get(index_handler)) }

async fn index_handler() -> Result<impl IntoResponse, WebError> {
	#[derive(Debug, Template)]
	#[template(path = "index.html.j2")]
	struct Tmpl<'a> {
		nonce: &'a str,
	}
	let nonce = rand::random::<u64>().to_string();

	let template = Tmpl { nonce: &nonce };
	Ok((
		[(header::CONTENT_SECURITY_POLICY, format!("default-src 'none' 'nonce-{nonce}';"))],
		Html(template.render()?),
	))
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
		struct Tmpl<'a> {
			nonce: &'a str,
			err: WebError,
		}

		let nonce = rand::random::<u64>().to_string();

		let status = match &self {
			| Self::Render(_) => StatusCode::INTERNAL_SERVER_ERROR,
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
