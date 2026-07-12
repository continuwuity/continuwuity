use axum::extract::State;
use conduwuit::{Err, Result};
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
		ACCOUNT_MANAGEMENT_PATH, AUTH_CODE_PATH, CLIENT_REGISTER_PATH, DEVICE_AUTHORIZATION_PATH,
		JWKS_URI_PATH, TOKEN_PATH, TOKEN_REVOKE_PATH,
	},
};

pub(crate) async fn get_authorization_server_metadata_route(
	State(services): State<crate::State>,
	_body: Ruma<get_authorization_server_metadata::v1::Request>,
) -> Result<get_authorization_server_metadata::v1::Response> {
	if !services.config.oauth.compatibility_mode().oauth_available() {
		return Err!(Request(Unrecognized("OAuth is unavailable on this server")));
	}

	let metadata = Raw::new(&authorization_server_metadata(&services).await).unwrap();

	Ok(get_authorization_server_metadata::v1::Response::new(metadata.cast_unchecked()))
}

pub(crate) async fn authorization_server_metadata(services: &Services) -> Value {
	let endpoint_base = services
		.config
		.get_client_domain()
		.join(super::BASE_PATH)
		.unwrap();

	let prompt_values_supported = if services
		.uiaa
		.registration_flow_status()
		.await
		.any_available()
	{
		json!(["create"])
	} else {
		json!([])
	};

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
		"device_authorization_endpoint": endpoint_base.join(DEVICE_AUTHORIZATION_PATH).unwrap(),
		"grant_types_supported": ["authorization_code", "refresh_token", "urn:ietf:params:oauth:grant-type:device_code"],
		"issuer": services.config.get_client_domain(),
		"jwks_uri": endpoint_base.join(JWKS_URI_PATH).unwrap(),
		"prompt_values_supported": prompt_values_supported,
		"registration_endpoint": endpoint_base.join(CLIENT_REGISTER_PATH).unwrap(),
		"response_modes_supported": ["query", "fragment"],
		"response_types_supported": ["code"],
		"revocation_endpoint": endpoint_base.join(TOKEN_REVOKE_PATH).unwrap(),
		"token_endpoint": endpoint_base.join(TOKEN_PATH).unwrap(),
	})
}
