use axum::{Json, extract::State};
use conduwuit::{Result, err};
use oxide_auth::primitives::prelude::Client;
use reqwest::Url;
use ruma::{DeviceId, identifiers_validation};

/// The required parameters to register a new client for OAuth2 application.
#[derive(serde::Deserialize, Clone, Debug)]
pub(crate) struct ClientQuery {
	/// Human-readable name.
	client_name: String,
	/// A public page that tells more about the client. All other links must be
	/// within.
	client_uri: Url,
	/// Redirect URIs declared by the client. At least one.
	redirect_uris: Vec<Url>,
	/// Must be `["code"]`.
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
#[derive(serde::Serialize, Debug)]
pub(crate) struct ClientResponse {
	client_id: String,
	client_secret: Option<String>,
	client_secret_expires_at: Option<u32>,
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
/// Register a client, as specified in [MSC2966]. This client, "device" in OIDC
/// parlance, will have the right to submit [super::authorize::authorize]
/// requests.
///
/// [MSC2966]: https://github.com/matrix-org/matrix-spec-proposals/pull/2966
pub(crate) async fn register_client(
	State(services): State<crate::State>,
	Json(client): Json<ClientQuery>,
) -> Result<Json<ClientResponse>> {
	tracing::trace!("processing OIDC device register request for client: {client:#?}");
	let Some(redirect_uri) = client.redirect_uris.first().cloned() else {
		return Err(err!(Request(Unknown(
			"register request should contain at least a redirect_uri"
		))));
	};
	let device_id = DeviceId::new();
	let scope = format!(
		"urn:matrix:org.matrix.msc2967.client:api:* \
		 urn:matrix:org.matrix.msc2967.client:device:{device_id}"
	).parse().expect("parseable default Matrix scope");
	// TODO check if the users service needs an update.
	//services.users.update_device_metadata();

	// If the client cannot authenticate itself at the token endpoint, then
	// it's a public client.
	let is_private = client.token_endpoint_auth_method != "none";
	// TODO generate a device secret.
	let secret = "cacestdubonsecretmonlouou=--".to_string();
	if let Err(err) = identifiers_validation::client_secret::validate(&secret) {
		tracing::warn!("oops, we generated an invalid client_secret: {err}");
	}
	let registerable = match is_private {
		| true => &Client::confidential(
			device_id.as_ref(),
			redirect_uri,
			scope,
			secret.as_bytes(),
		).with_additional_redirect_uris(remaining_uris),
		| _ => &Client::public(
			device_id.as_ref(),
			redirect_uri,
			scope,
		).with_additional_redirect_uris(remaining_uris)
	};
	tracing::trace!("registering OIDC device : {registerable:#?}");
	services.oidc.register_client(&registerable)?;

	let client_response = ClientResponse {
		client_id: device_id.to_string(),
		client_secret: if is_private { Some(secret) } else { None },
		client_secret_expires_at: if is_private { Some(0) } else { None },
		client_name: client.client_name.clone(),
		client_uri: client.client_uri.clone(),
		redirect_uris: client.redirect_uris.clone(),
		logo_uri: client.logo_uri.clone(),
		policy_uri: client.policy_uri.clone(),
		tos_uri: client.tos_uri.clone(),
		token_endpoint_auth_method: client.token_endpoint_auth_method.clone(),
		response_types: client.response_types.clone(),
		grant_types: client.grant_types.clone(),
		application_type: client.application_type,
	};
	tracing::debug!("OIDC device registered : {client_response:#?}");

	Ok(Json(client_response))
}
