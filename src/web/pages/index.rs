use askama::Template;
use axum::{
	Router,
	extract::State,
	response::{Html, IntoResponse},
	routing::get,
};
use conduwuit_service::state;

use crate::WebError;

pub(crate) fn build() -> Router<state::State> { Router::new().route("/", get(index_handler)) }

async fn index_handler(
	State(services): State<state::State>,
) -> Result<impl IntoResponse, WebError> {
	#[derive(Debug, Template)]
	#[template(path = "index.html.j2")]
	struct Index<'a> {
		server_name: &'a str,
		first_run: bool,
	}

	let template = Index {
		server_name: services.config.server_name.as_str(),
		first_run: services.firstrun.is_first_run(),
	};
	Ok(Html(template.render()?))
}
