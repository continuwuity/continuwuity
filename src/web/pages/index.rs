use askama::Template;
use axum::{Router, extract::State, response::IntoResponse, routing::get};

use crate::{WebError, template};

pub(crate) fn build() -> Router<crate::State> { Router::new().route("/", get(index_handler)) }

async fn index_handler(
	State(services): State<crate::State>,
) -> Result<impl IntoResponse, WebError> {
	template! {
		struct Index<'a> use "index.html.j2" {
			server_name: &'a str,
			first_run: bool
		}
	}

	Ok(Index::new(
		&services,
		services.globals.server_name().as_str(),
		services.firstrun.is_first_run(),
	)
	.into_response())
}
