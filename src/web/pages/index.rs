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
		server_name: &'a str,
		first_run: bool,
		allow_indexing: bool,
	}

	let template = Index {
		server_name: services.globals.server_name().as_str(),
		first_run: services.firstrun.is_first_run(),
		allow_indexing: services.config.index_page_allow_indexing,
	};
	Ok(Html(template.render()?))
}
