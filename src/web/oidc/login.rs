use super::{
	AuthorizationQuery,
	LoginPageTemplate,
	OidcRequest,
	OidcResponse,
};
use askama::Template;
use oxide_auth::{
	endpoint::QueryParameter,
	frontends::simple::request::{Body, Status},
};
use url::Url;
use std::str::FromStr;


/// The set of query parameters a client needs to get authorization.
#[derive(serde::Deserialize, Debug, Clone)]
pub struct LoginQuery {
	pub username: String,
	pub password: String,
	pub client_id: String,
	pub redirect_uri: Url,
	pub scope: String,
	pub state: String,
	pub code_challenge: String,
	pub code_challenge_method: String,
	pub response_type: String,
	pub response_mode: String,
}

#[derive(Debug)]
pub struct LoginError(pub String);

impl TryFrom<OidcRequest> for LoginQuery {
	type Error = LoginError;

	fn try_from(value: OidcRequest) -> Result<Self, LoginError> {
		let body = value.body().expect("body in OidcRequest");

		let Some(username) = body.unique_value("username") else {
			return Err(LoginError("missing field: username".to_string()));
		};
		let Some(password) = body.unique_value("password") else {
			return Err(LoginError("missing field: password".to_string()));
		};
		let Some(client_id) = body.unique_value("client_id") else {
			return Err(LoginError("missing field: client_id".to_string()));
		};
		let Some(redirect_uri) = body.unique_value("redirect_uri") else {
			return Err(LoginError("missing field: redirect_uri".to_string()));
		};
		let Some(scope) = body.unique_value("scope") else {
			return Err(LoginError("missing field: scope".to_string()));
		};
		let Some(state) = body.unique_value("state") else {
			return Err(LoginError("missing field: state".to_string()));
		};
		let Some(code_challenge) = body.unique_value("code_challenge") else {
			return Err(LoginError("missing field: code_challenge".to_string()));
		};
		let Some(code_challenge_method) = body.unique_value("code_challenge_method") else {
			return Err(LoginError("missing field: code_challenge_method".to_string()));
		};
		let Some(response_type) = body.unique_value("response_type") else {
			return Err(LoginError("missing field: response_type".to_string()));
		};
		let Some(response_mode) = body.unique_value("response_mode") else {
			return Err(LoginError("missing field: response_mode".to_string()));
		};
		let Ok(redirect_uri) = Url::from_str(&redirect_uri) else {
			return Err(LoginError("invalid field: redirect_uri".to_string()));
		};

		Ok(LoginQuery {
			username: username.to_string(),
			password: password.to_string(),
			client_id: client_id.to_string(),
			redirect_uri,
			scope: scope.to_string(),
			state: state.to_string(),
			code_challenge: code_challenge.to_string(),
			code_challenge_method: code_challenge_method.to_string(),
			response_type: response_type.to_string(),
			response_mode: response_mode.to_string(),
		})
	}
}

/// A web login form for the OIDC authentication flow.
///
/// The returned `OidcResponse` handles CSP headers to allow that form.
pub fn oidc_login_form(
	hostname: &str,
	query: &AuthorizationQuery,
) -> OidcResponse {
	// The target request route.
	let route = "/_matrix/client/unstable/org.matrix.msc2964/login";
	let nonce = rand::random::<u64>().to_string();
	let body = Some(Body::Text(login_page(hostname, query, route, &nonce)));

	OidcResponse {
		status: Status::Ok,
		location: None,
		www_authenticate: None,
		body,
		nonce,
	}
}

/// Render the html contents of the login page.
fn login_page(
	hostname: &str,
	query: &AuthorizationQuery,
	route: &str,
	nonce: &str,
) -> String {
	let template = LoginPageTemplate {
		nonce,
		hostname,
		route,
		client_id: query.client_id.as_str(),
		redirect_uri: query.redirect_uri.as_str(),
		scope: query.scope.as_str(),
		state: query.state.as_str(),
		code_challenge: query.code_challenge.as_str(),
		code_challenge_method: query.code_challenge_method.as_str(),
		response_type: query.response_type.as_str(),
		response_mode: query.response_mode.as_str(),
	};

	template.render().expect("login template render")
}
