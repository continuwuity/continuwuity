use super::{OidcRequest, oidc_consent_form};

use crate::oidc::LoginQuery;
use oxide_auth::{
	endpoint::{OwnerConsent, OwnerSolicitor, Solicitation, WebRequest, WebResponse},
	frontends::simple::request::{Body as OAuthRequestBody, Status},
};
use oxide_auth_axum::WebError;
use axum::{
	body::Body,
    http::{Response, header},
	response::IntoResponse,
};
use url::Url;

/// A Web response that can be processed by the OIDC authentication flow before
/// being sent over.
#[derive(Default, Clone, Debug)]
pub struct OidcResponse {
	pub(crate) status: Status,
	pub(crate) location: Option<Url>,
	pub(crate) www_authenticate: Option<String>,
	pub(crate) body: Option<OAuthRequestBody>,
	pub(crate) nonce: String,
}

impl OidcResponse {
	/// Instanciate from a response body. Used to send login or consent forms.
	pub fn from_body(body: &str) -> Result<Self, WebError> {
		let mut result = OidcResponse::default();
		result.body_text(body)?;

		Ok(result)
	}
}

impl IntoResponse for OidcResponse {
	fn into_response(self) -> Response<Body> {
		let body = self.body.expect("body").as_str().to_string();
		let response = Response::builder()
			.header(header::CONTENT_TYPE, "text/html")
			.header(
				header::CONTENT_SECURITY_POLICY,
				format!("default-src 'nonce-{}'; form-action https://eon.presentmatter.one/;", self.nonce)
			)
			.body(body.into())
			.unwrap();

		response
	}
}

impl OwnerSolicitor<OidcRequest> for OidcResponse {
	fn check_consent(
		&mut self,
		request: &mut OidcRequest,
		_: Solicitation<'_>,
	) -> OwnerConsent<<OidcRequest as WebRequest>::Response> {
		//let hostname = self.location.map(|l| l.as_str()).unwrap_or("Continuwuity");
		let hostname = "Continuwuity";
		/*
		let hostname = request
			.query()
			.expect("query in OAuth request")
			.unique_value("hostname")
			.expect("hostname in OAuth request")
			.as_str();
		*/
		let query: LoginQuery = request
			.clone()
			.try_into()
			.expect("login query from OidcRequest");

		OwnerConsent::InProgress(oidc_consent_form(
			hostname,
			&query.into(),
		))
	}
}

impl WebResponse for OidcResponse {
	type Error = WebError;

	fn ok(&mut self) -> Result<(), Self::Error> {
		self.status = Status::Ok;
		self.location = None;
		self.www_authenticate = None;

		Ok(())
	}

    /// A response which will redirect the user-agent to which the response is issued.
    fn redirect(&mut self, url: Url) -> Result<(), Self::Error> {
        self.status = Status::Redirect;
        self.location = Some(url);
        self.www_authenticate = None;
        Ok(())
    }

    /// Set the response status to 400.
    fn client_error(&mut self) -> Result<(), Self::Error> {
        self.status = Status::BadRequest;
        self.location = None;
        self.www_authenticate = None;
        Ok(())
    }

    /// Set the response status to 401 and add a `WWW-Authenticate` header.
    fn unauthorized(&mut self, header_value: &str) -> Result<(), Self::Error> {
        self.status = Status::Unauthorized;
        self.location = None;
        self.www_authenticate = Some(header_value.to_owned());
        Ok(())
    }

    /// A pure text response with no special media type set.
    fn body_text(&mut self, text: &str) -> Result<(), Self::Error> {
        self.body = Some(OAuthRequestBody::Text(text.to_owned()));
        Ok(())
    }

    /// Json repsonse data, with media type `aplication/json.
    fn body_json(&mut self, data: &str) -> Result<(), Self::Error> {
        self.body = Some(OAuthRequestBody::Json(data.to_owned()));
        Ok(())
    }
}
