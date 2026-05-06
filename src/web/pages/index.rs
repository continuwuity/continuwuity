use axum::{Extension, Router, extract::State, routing::get};

use crate::{
	pages::{Result, TemplateContext},
	response, template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/", get(index))
		.route(&format!("{}/", crate::ROUTE_PREFIX), get(index))
}

template! {
	struct Index<'a> use "index.html.j2" {
		server_name: &'a str,
		first_run: bool
	}
}

async fn index(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
) -> Result {
	response!(Index::new(
		context,
		services.globals.server_name().as_str(),
		services.firstrun.is_first_run(),
	))
}
