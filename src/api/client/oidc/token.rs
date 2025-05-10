use axum::extract::State;
use conduwuit::{Result, err};
use conduwuit_web::oidc::{OidcRequest, OidcResponse};
use oxide_auth::endpoint::QueryParameter;

/// # `POST /_matrix/client/unstable/org.matrix.msc2964/token`
///
/// Depending on `grant_type`, either deliver a new token to a device, and store
/// it in the server's ring, or refresh the token.
pub(crate) async fn token(
	State(services): State<crate::State>,
	oauth: OidcRequest,
) -> Result<OidcResponse> {
	let Some(body) = oauth.body() else {
		return Err(err!(Request(Unknown("OAuth request had an empty body"))));
	};
	let grant_type = body
		.unique_value("grant_type")
		.map(|value| value.to_string());
	let endpoint = services.oidc.endpoint();

	match grant_type.as_deref() {
		| Some("authorization_code") => endpoint
			.access_token_flow()
			.execute(oauth)
			.map_err(|err| err!(Request(Unknown("token grant failed: {err:?}")))),
		| Some("refresh_token") => endpoint
			.refresh_flow()
			.execute(oauth)
			.map_err(|err| err!(Request(Unknown("token refresh failed: {err:?}")))),
		| other => Err(err!(Request(Unknown("unsupported grant type: {other:?}")))),
	}
}
