use oxide_auth::primitives::prelude::Client;
use axum::{
	Json,
	extract::State,
};
use conduwuit::{Result, err};
use ruma::DeviceId;
use reqwest::Url;

/// The required parameters to register a new client for OAuth2 application.
#[derive(serde::Deserialize, Clone)]
pub(crate) struct ClientQuery {
	/// Human-readable name.
    client_name: String,
	/// A public page that tells more about the client. All other links must be within.
    client_uri: Url,
	/// Redirect URIs declared by the client. At least one.
	redirect_uris: Vec<Url>,
	/// Must be ["code"].
	response_types: Vec<String>,
	/// Must include "authorization_type" and "refresh_token".
	grant_types: Vec<String>,
	//contacts: Vec<String>,
	/// Can be "none".
	token_endpoint_auth_method: String,
	/// Link to the logo.
	logo_uri: Option<Url>,
	/// Link to the client's policy.
	policy_uri: Option<Url>,
	/// Link to the terms of service.
	tos_uri: Option<Url>,
	/// Defaults to "web" if not present.
	application_type: Option<String>,
}

/// A successful response that the client was registered.
#[derive(serde::Serialize)]
pub(crate) struct ClientResponse {
	client_id: String,
	client_name: String,
	client_uri: Url,
	logo_uri: Option<Url>,
	tos_uri: Option<Url>,
	policy_uri: Option<Url>,
	redirect_uris: Vec<Url>,
	token_endpoint_auth_method: String,
	response_types: Vec<String>,
	grant_types: Vec<String>,
	application_type: Option<String>,
}

/// # `GET /_matrix/client/unstable/org.matrix.msc2964/device/register`
///
/// Register a client, as specified in [MSC2966]. This client, "device" in OIDC parlance,
/// will have the right to submit [super::authorize::authorize] requests.
///
/// [MSC2966]: https://github.com/matrix-org/matrix-spec-proposals/pull/2966
pub(crate) async fn register_client(
	State(services): State<crate::State>,
	Json(client): Json<ClientQuery>,
) -> Result<Json<ClientResponse>> {
	let Some(redirect_uri) = client.redirect_uris.first().cloned() else {
		return Err(err!(Request(Unknown(
			"register request should contain at least a redirect_uri"
		))));
	};
	let device_id = DeviceId::new();
	let scope = format!(
		"urn:matrix:org.matrix.msc2967.client:api:* urn:matrix:org.matrix.msc2967.client:device:{}",
		device_id
	);
	// TODO check if the users service needs an update.
	//services.users.update_device_metadata();
	services.oidc.register_client(&Client::public(
		&device_id.to_string(),
		redirect_uri.into(),
		scope.parse().expect("device ID should parse in Matrix scope"),
	))?;

	Ok(Json(ClientResponse {
		client_id: device_id.to_string(),
		client_name: client.client_name.clone(),
		client_uri: client.client_uri.clone(),
		redirect_uris: client.redirect_uris.clone(),
		logo_uri: client.logo_uri.clone(),
		policy_uri: client.policy_uri.clone(),
		tos_uri: client.tos_uri.clone(),
		token_endpoint_auth_method: client.token_endpoint_auth_method.clone(),
		response_types: client.response_types.clone(),
		grant_types: client.grant_types.clone(),
		application_type: client.application_type.clone(),
	}))
}
