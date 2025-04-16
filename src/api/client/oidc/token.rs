/// Implementation of [MSC2964]'s OAuth2 restricted flow using the [oxide-auth]
/// crate. See the MSC for restrictions that apply to this flow.
///
/// [MSC2965]: https://github.com/matrix-org/matrix-spec-proposals/pull/2965
/// [oxide-auth]: https://docs.rs/oxide-auth

use oxide_auth_axum::{OAuthResponse, OAuthRequest};
use oxide_auth::endpoint::QueryParameter;
use axum::{
	extract::State,
	response::IntoResponse,
};
use conduwuit::{Result, err};

/// # `POST /_matrix/client/unstable/org.matrix.msc2964/token`
///
/// Depending on `grant_type`, either deliver a new token to a device, and store
/// it in the server's ring, or refresh the token.
pub(crate) async fn token(
	State(services): State<crate::State>,
	oauth: OAuthRequest,
) -> Result<OAuthResponse> {
	let Some(body) = oauth.body() else {
		return Err(err!(Request(Unknown("OAuth request had an empty body"))));
	};
	let grant_type = body
		.unique_value("grant_type")
		.map(|value| value.to_string());
	let endpoint = services.oidc.endpoint();

	match grant_type.as_deref() {
		| Some("authorization_code") =>
			endpoint
				.access_token_flow()
				.execute(oauth)
				.map_err(|err| err!(Request(Unknown("token grant failed: {err:?}")))),
		| Some("refresh_token") =>
			endpoint
				.refresh_flow()
				.execute(oauth)
				.map_err(|err| err!(Request(Unknown("token refresh failed: {err:?}")))),
		| other =>
			Err(err!(Request(Unknown("unsupported grant type: {other:?}")))),
	}
}

/// Sample protected content. TODO check that resources are available with the returned token.
pub(crate) async fn _protected_resource(
	State(services): State<crate::State>,
	oauth: OAuthRequest,
) -> impl IntoResponse {
	const DENY_TEXT: &str = "<html>
This page should be accessed via an oauth token from the client in the example. Click
<a href=\"/authorize?response_type=code&client_id=LocalClient\">
here</a> to begin the authorization process.
</html>
";

	let protect = services
		.oidc
		.endpoint()
		.with_scopes(vec!["default-scope".parse().unwrap()])
		.resource_flow()
		.execute(oauth);
	match protect {
		Ok(_grant) => Ok("Hello, world"),
		Err(Ok(response)) => {
			let error: OAuthResponse = response
				//.header(ContentType::HTML)
				.body(DENY_TEXT)
				//.finalize()
				.into();
			Err(Ok(error))
		}
		Err(Err(err)) => Err(Err(err!(Request(Unknown("auth failed: {err:?}"))))),
	}
}

