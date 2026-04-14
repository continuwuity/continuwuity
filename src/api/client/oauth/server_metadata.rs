use axum::extract::State;
use conduwuit::Result;
use ruma::{api::client::discovery::get_authorization_server_metadata, serde::Raw};
use serde_json::json;

use crate::Ruma;

pub(crate) async fn get_authorization_server_metadata_route(
	State(services): State<crate::State>,
	_body: Ruma<get_authorization_server_metadata::v1::Request>,
) -> Result<get_authorization_server_metadata::v1::Response> {
	let endpoint_base = services
		.config
		.get_client_domain()
		.join(super::BASE_PATH)
		.unwrap();

	let metadata = Raw::new(&json!({
		"authorization_endpoint": endpoint_base.join("grant/authorization_code").unwrap(),
		"code_challenge_methods_supported": ["S256"],
		"grant_types_supported": ["authorization_code", "refresh_token"],
		"issuer": services.config.get_client_domain(),
		"jwks_uri": endpoint_base.join("client/keys.json").unwrap(),
		"prompt_values_supported": ["create"],
		"registration_endpoint": endpoint_base.join("client/register").unwrap(),
		"response_modes_supported": ["query", "fragment"],
		"response_types_supported": ["code"],
		"revocation_endpoint": endpoint_base.join("client/revoke").unwrap(),
		"token_endpoint": endpoint_base.join("grant/token").unwrap(),
	}))
	.unwrap();

	Ok(get_authorization_server_metadata::v1::Response::new(metadata.cast_unchecked()))
}
