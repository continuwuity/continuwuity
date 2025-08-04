use askama::Template;
use axum::http::StatusCode;
use oxide_auth::frontends::simple::request::Body;

use super::{AuthorizationQuery, ConsentPageTemplate, OidcResponse, encode};

/// A web consent solicitor form for the OIDC authentication flow.
///
/// Asks the resource owner for their consent to let a client access their data
/// on this server.
#[must_use]
pub fn oidc_consent_form(hostname: &str, query: &AuthorizationQuery) -> OidcResponse {
	// The target request route.
	let route = "/_matrix/client/unstable/org.matrix.msc2964/authorize";
	let nonce = rand::random::<u64>().to_string();
	let body = Some(Body::Text(consent_page(hostname, query, route, &nonce)));

	OidcResponse {
		status: StatusCode::OK,
		location: None,
		www_authenticate: None,
		body,
		nonce: Some(nonce),
	}
}

/// Render the html contents of the user consent page.
fn consent_page(hostname: &str, query: &AuthorizationQuery, route: &str, nonce: &str) -> String {
	let response_mode = &query.response_mode.clone()
		.unwrap_or_else(|| match query.redirect_uri.scheme() {
			| "https" => "fragment",
			| _ => "query"
		});
	let template = ConsentPageTemplate {
		nonce,
		hostname,
		route,
		client_id: &encode(query.client_id.as_str()),
		redirect_uri: &encode(query.redirect_uri.as_str()),
		scope: &encode(query.scope.as_str()),
		state: &encode(query.state.as_str()),
		code_challenge: &encode(query.code_challenge.as_str()),
		code_challenge_method: &encode(query.code_challenge_method.as_str()),
		response_type: &encode(query.response_type.as_str()),
		response_mode: &encode(response_mode),
	};

	template.render().expect("consent page render")
}
