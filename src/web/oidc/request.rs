use std::borrow::Cow;

use async_trait::async_trait;
use axum::{
	extract::{Form, FromRequest, FromRequestParts, Query, Request},
	http::header,
};
use oxide_auth::endpoint::{NormalizedParameter, QueryParameter, WebRequest};

use super::{OidcError, OidcResponse};

/// An OIDC authentication request.
///
/// Expected to receive GET and POST requests to the `authorize` endpoint, or
/// POST requests to the `login` endpoint.
///
/// Mostly adapted from the OAuthRequest struct in the [oxide-auth-axum] crate.
/// [oxide-auth-axum]: https://docs.rs/oxide-auth-axum
#[derive(Clone, Debug)]
pub struct OidcRequest {
	pub(crate) auth: Option<String>,
	pub(crate) query: Option<NormalizedParameter>,
	pub(crate) body: Option<NormalizedParameter>,
}

impl OidcRequest {
	/// Fetch the authorization header from the request
	#[must_use]
	pub fn authorization_header(&self) -> Option<&str> { self.auth.as_deref() }

	/// Fetch the query for this request
	#[must_use]
	pub fn query(&self) -> Option<&NormalizedParameter> { self.query.as_ref() }

	/// Fetch the query mutably
	pub fn query_mut(&mut self) -> Option<&mut NormalizedParameter> { self.query.as_mut() }

	/// Fetch the body of the request
	#[must_use]
	pub fn body(&self) -> Option<&NormalizedParameter> { self.body.as_ref() }
}

impl WebRequest for OidcRequest {
	type Error = OidcError;
	type Response = OidcResponse;

	fn query(&mut self) -> Result<Cow<'_, dyn QueryParameter + 'static>, Self::Error> {
		self.query
			.as_ref()
			.map(|q| {
				let q: &dyn QueryParameter = q;
				Cow::Borrowed(q)
			})
			.ok_or(OidcError::Query)
	}

	fn urlbody(&mut self) -> Result<Cow<'_, dyn QueryParameter + 'static>, Self::Error> {
		self.body
			.as_ref()
			.map(|q| {
				let q: &dyn QueryParameter = q;
				Cow::Borrowed(q)
			})
			.ok_or(OidcError::Body)
	}

	fn authheader(&mut self) -> Result<Option<Cow<'_, str>>, Self::Error> {
		Ok(self.auth.as_deref().map(Cow::Borrowed))
	}
}

#[async_trait]
impl<S> FromRequest<S> for OidcRequest
where
	S: Send + Sync,
{
	type Rejection = OidcError;

	async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
		let mut all_auth = req.headers().get_all(header::AUTHORIZATION).iter();
		let optional = all_auth.next();

		let auth = if all_auth.next().is_some() {
			return Err(OidcError::Authorization);
		} else {
			optional.and_then(|hv| hv.to_str().ok().map(str::to_owned))
		};

		let (mut parts, body) = req.into_parts();
		let query = Query::from_request_parts(&mut parts, state)
			.await
			.ok()
			.map(|q: Query<NormalizedParameter>| q.0);

		let req = Request::from_parts(parts, body);
		let body = Form::from_request(req, state)
			.await
			.ok()
			.map(|b: Form<NormalizedParameter>| b.0);

		// If the query is empty and the body has a request, copy it over
		// because login forms are POST requests but OAuth flow expects
		// arguments in query.
		let query = match query {
			| None => body.clone(),
			| Some(params) => {
				//if params == NormalizedParameter::new() {
				if params.unique_value("client_id").is_none() {
					body.clone()
				} else {
					Some(params)
				}
			},
		};

		Ok(Self { auth, query, body })
	}
}
