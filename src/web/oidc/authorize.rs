use url::Url;

use super::LoginQuery;

/// The set of parameters required for an OIDC authorization request.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct AuthorizationQuery {
	pub client_id: String,
	pub client_secret: Option<String>,
	pub redirect_uri: Url,
	pub scope: String,
	pub state: String,
	pub code_challenge: String,
	pub code_challenge_method: String,
	pub response_type: String,
	pub response_mode: Option<String>,
	pub username: Option<String>,
}

impl From<LoginQuery> for AuthorizationQuery {
	fn from(value: LoginQuery) -> Self {
		let LoginQuery {
			client_id,
			client_secret,
			redirect_uri,
			scope,
			state,
			code_challenge,
			code_challenge_method,
			response_type,
			response_mode,
			username,
			..
		} = value;

		Self {
			client_id,
			client_secret,
			redirect_uri,
			scope,
			state,
			code_challenge,
			code_challenge_method,
			response_type,
			response_mode: Some(response_mode),
			username: Some(username),
		}
	}
}
