mod register_client;
mod server_metadata;
mod token;

use axum::{
	Json, Router,
	extract::State,
	routing::method_routing::{get, post},
};
use serde_json::json;
pub(crate) use server_metadata::*;

const BASE_PATH: &str = const_str::concat!(conduwuit_core::ROUTE_PREFIX, "/oauth2/");

pub(crate) fn router() -> Router<crate::State> {
	Router::new().nest(BASE_PATH, oauth_router())
	// TODO(unspecced): used by old versions of the matrix-js-sdk
	.route("/.well-known/openid-configuration", get(
		async |State(services): State<crate::State>| {
			Json(authorization_server_metadata(&services).await)
		}
	))
}

fn oauth_router() -> Router<crate::State> {
	Router::new()
		.route(CLIENT_REGISTER_PATH, post(register_client::register_client_route))
		// TODO(unspecced): used by old versions of the matrix-js-sdk
		.route(JWKS_URI_PATH, get(async || Json(json!({"keys": []}))))
		.route(TOKEN_PATH, post(token::token_route))
		.route(TOKEN_REVOKE_PATH, post(token::revoke_token_route))
}
