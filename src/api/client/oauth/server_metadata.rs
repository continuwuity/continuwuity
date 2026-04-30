use axum::extract::State;
use conduwuit::Result;
use ruma::{
	api::client::discovery::get_authorization_server_metadata::{
		self, v1::AccountManagementAction,
	},
	serde::Raw,
};
use serde_json::{Value, json};
use service::Services;

use crate::{
	Ruma,
	client::oauth::{
		ACCOUNT_MANAGEMENT_PATH, AUTH_CODE_PATH, CLIENT_REGISTER_PATH, JWKS_URI_PATH, TOKEN_PATH,
		TOKEN_REVOKE_PATH,
	},
};

pub(crate) async fn get_authorization_server_metadata_route(
	State(services): State<crate::State>,
	_body: Ruma<get_authorization_server_metadata::v1::Request>,
) -> Result<get_authorization_server_metadata::v1::Response> {
	let metadata = Raw::new(&authorization_server_metadata(&services).await).unwrap();

	Ok(get_authorization_server_metadata::v1::Response::new(metadata.cast_unchecked()))
}

pub(crate) async fn authorization_server_metadata(services: &Services) -> Value {
	let endpoint_base = services
		.config
		.get_client_domain()
		.join(super::BASE_PATH)
		.unwrap();

	json!({
		"account_management_uri": endpoint_base.join(ACCOUNT_MANAGEMENT_PATH).unwrap(),
		"account_management_actions_supported": [
			AccountManagementAction::AccountDeactivate,
			AccountManagementAction::CrossSigningReset,
			AccountManagementAction::DeviceDelete,
			AccountManagementAction::DeviceView,
			AccountManagementAction::DevicesList,
			AccountManagementAction::Profile,
		],
		"authorization_endpoint": endpoint_base.join(AUTH_CODE_PATH).unwrap(),
		"code_challenge_methods_supported": ["S256"],
		"grant_types_supported": ["authorization_code", "refresh_token"],
		"issuer": services.config.get_client_domain(),
		"jwks_uri": endpoint_base.join(JWKS_URI_PATH).unwrap(),
		"prompt_values_supported": ["create"],
		"registration_endpoint": endpoint_base.join(CLIENT_REGISTER_PATH).unwrap(),
		"response_modes_supported": ["query", "fragment"],
		"response_types_supported": ["code"],
		"revocation_endpoint": endpoint_base.join(TOKEN_REVOKE_PATH).unwrap(),
		"token_endpoint": endpoint_base.join(TOKEN_PATH).unwrap(),
	})
}
