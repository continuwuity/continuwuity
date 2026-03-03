use askama::Template;
use axum::{
	Router,
	extract::State,
	response::{Html, IntoResponse},
	routing::get,
};

use crate::WebError;

pub(crate) fn build() -> Router<crate::State> { Router::new().route("/", get(index_handler)) }

async fn index_handler(
	State(services): State<crate::State>,
) -> Result<impl IntoResponse, WebError> {
	#[derive(Debug, Template)]
	#[template(path = "index.html.j2")]
	struct Index<'a> {
		client_domain: &'a str,
		first_run: bool,
	}

	let client_domain = services.config.get_client_domain();
	let host = client_domain
		.host_str()
		.expect("client domain should have a host");
	let client_domain = if let Some(port) = client_domain.port() {
		&format!("{host}:{port}")
	} else {
		host
	};

	let template = Index {
		client_domain,
		first_run: services.firstrun.is_first_run(),
	};
	Ok(Html(template.render()?))
}
