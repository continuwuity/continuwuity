use askama::Template;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

// Imports needed by askama templates.
use crate::{GIT_REMOTE_COMMIT_URL, GIT_REMOTE_WEB_URL, VERSION_EXTRA};

mod authorize;
mod consent;
mod error;
mod login;
mod request;
mod response;
pub use authorize::AuthorizationQuery;
pub use consent::oidc_consent_form;
pub use error::OidcError;
pub use login::{LoginError, LoginQuery, oidc_login_form};
pub use request::OidcRequest;
pub use response::OidcResponse;

/// The parameters for the OIDC login page template.
#[derive(Template)]
#[template(path = "login.html.j2")]
pub(crate) struct LoginPageTemplate<'a> {
	nonce: &'a str,
	hostname: &'a str,
	route: &'a str,
	client_id: &'a str,
	redirect_uri: &'a str,
	scope: &'a str,
	state: &'a str,
	code_challenge: &'a str,
	code_challenge_method: &'a str,
	response_type: &'a str,
	response_mode: &'a str,
}

/// The parameters for the OIDC consent page template.
#[derive(Template)]
#[template(path = "consent.html.j2")]
pub(crate) struct ConsentPageTemplate<'a> {
	nonce: &'a str,
	hostname: &'a str,
	route: &'a str,
	client_id: &'a str,
	redirect_uri: &'a str,
	scope: &'a str,
	state: &'a str,
	code_challenge: &'a str,
	code_challenge_method: &'a str,
	response_type: &'a str,
	response_mode: &'a str,
}

pub(crate) fn encode(text: &str) -> String {
	utf8_percent_encode(text, NON_ALPHANUMERIC).to_string()
}
