mod register_client;
mod server_metadata;

use axum::{
	Json, Router,
	routing::method_routing::{get, post},
};
use serde_json::json;
pub(crate) use server_metadata::*;

pub(crate) const BASE_PATH: &str = "/_continuwuity/oauth2/";

pub(crate) fn router() -> Router<crate::State> {
	Router::new()
		.route("/client/register", post(register_client::register_client_route))
		.route("/client/keys.json", get(async || Json(json!({"keys": []}))))
}
