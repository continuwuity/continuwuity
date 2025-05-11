use axum::{
	body::Body,
	http::{Response, StatusCode, header},
	response::IntoResponse,
};
use oxide_auth::{
	endpoint::{OwnerConsent, OwnerSolicitor, Solicitation, WebRequest, WebResponse},
	frontends::simple::request::Body as OAuthRequestBody,
};
use url::Url;

use super::{LoginQuery, OidcError, OidcRequest, oidc_consent_form};

/// A Web response that can be processed by the OIDC authentication flow before
/// being sent over.
#[derive(Default, Clone, Debug)]
pub struct OidcResponse {
	pub(crate) status: StatusCode,
	pub(crate) location: Option<Url>,
	pub(crate) www_authenticate: Option<String>,
	pub(crate) body: Option<OAuthRequestBody>,
	pub(crate) nonce: Option<String>,
}

impl IntoResponse for OidcResponse {
	fn into_response(self) -> Response<Body> {
		let content_csp = match self.nonce {
			| Some(nonce) => &format!("default-src 'nonce-{}'; form-action 'self';", nonce),
			| None => "default-src 'none'; form-action 'self';"
		};
		let content_type = match self.body {
			| Some(OAuthRequestBody::Json(_)) => "application/json",
			| _ => "text/html",
		};
		let mut response = Response::builder()
			.status(self.status)
			.header(header::CONTENT_TYPE, content_type)
			.header(header::CONTENT_SECURITY_POLICY, content_csp);
		if let Some(location) = self.location {
			response = response.header(header::LOCATION, location.as_str());
		}
		// Transform from OAuthRequestBody to String.
		let body_content = self
			.body
			.map(|b| b.as_str().to_string())
			.unwrap_or_default();

		response
			.body(body_content.into())
			.unwrap()
	}
}

/// OidcResponse uses [super::oidc_consent_form] to be turned into an owner
/// consent solicitation.
impl OwnerSolicitor<OidcRequest> for OidcResponse {
	fn check_consent(
		&mut self,
		request: &mut OidcRequest,
		_: Solicitation<'_>,
	) -> OwnerConsent<<OidcRequest as WebRequest>::Response> {
		// TODO find a way to pass the hostname to the template.
		let hostname = "Continuwuity";
		let query: LoginQuery = request
			.clone()
			.try_into()
			.expect("login query from OidcRequest");

		OwnerConsent::InProgress(oidc_consent_form(hostname, &query.into()))
	}
}

impl WebResponse for OidcResponse {
	type Error = OidcError;

	fn ok(&mut self) -> Result<(), Self::Error> {
		self.status = StatusCode::OK;

		Ok(())
	}

	/// A response which will redirect the user-agent to which the response is
	/// issued.
	fn redirect(&mut self, url: Url) -> Result<(), Self::Error> {
		self.status = StatusCode::FOUND;
		self.location = Some(url);

		Ok(())
	}

	/// Set the response status to 400.
	fn client_error(&mut self) -> Result<(), Self::Error> {
		self.status = StatusCode::BAD_REQUEST;

		Ok(())
	}

	/// Set the response status to 401 and add a `WWW-Authenticate` header.
	fn unauthorized(&mut self, header_value: &str) -> Result<(), Self::Error> {
		self.status = StatusCode::UNAUTHORIZED;
		self.www_authenticate = Some(header_value.to_owned());

		Ok(())
	}

	/// A pure text response with no special media type set.
	fn body_text(&mut self, text: &str) -> Result<(), Self::Error> {
		self.body = Some(OAuthRequestBody::Text(text.to_owned()));

		Ok(())
	}

	/// Json response data, with media type `aplication/json.
	fn body_json(&mut self, data: &str) -> Result<(), Self::Error> {
		self.body = Some(OAuthRequestBody::Json(data.to_owned()));

		Ok(())
	}
}
