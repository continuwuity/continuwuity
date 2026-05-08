use axum::{Extension, Router, extract::State, routing::get};

use crate::{
	pages::{Result, TemplateContext},
	response, template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/", get(get_index))
		.route(&format!("{}/", crate::ROUTE_PREFIX), get(get_index))
		.route(&format!("{}/_book", crate::ROUTE_PREFIX), get(get_book))
}

template! {
	struct Index<'a> use "index.html.j2" {
		server_name: &'a str,
		first_run: bool
	}
}

async fn get_index(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
) -> Result {
	response!(Index::new(
		context,
		services.globals.server_name().as_str(),
		services.firstrun.is_first_run(),
	))
}

template! {
	struct Book use "book.html.j2" {}
}

async fn get_book(Extension(context): Extension<TemplateContext>) -> Result {
	response!(Book::new(context))
}
