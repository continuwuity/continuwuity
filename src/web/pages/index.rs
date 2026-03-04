use askama::Template;
use axum::{
	Router,
	extract::State,
	response::{Html, IntoResponse, Redirect},
	routing::get,
};

use crate::WebError;

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/", get(async || Redirect::permanent("/_continuwuity/")))
		.route("/_continuwuity/", get(index_handler))
}

async fn index_handler(
	State(services): State<crate::State>,
) -> Result<impl IntoResponse, WebError> {
	#[derive(Debug, Template)]
	#[template(path = "index.html.j2")]
	struct Index<'a> {
		server_name: &'a str,
		first_run: bool,
	}

	let template = Index {
		server_name: services.globals.server_name().as_str(),
		first_run: services.firstrun.is_first_run(),
	};
	Ok(Html(template.render()?))
}
