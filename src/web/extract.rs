use axum::{
	extract::{FromRequest, FromRequestParts, Request},
	http::{Method, request::Parts},
};
use serde::de::DeserializeOwned;

use crate::WebError;

/// An extractor which deserializes a struct from a POST request's body.
/// For GET requests the struct will be None.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
pub(crate) struct PostForm<T>(pub Option<T>);

impl<T, S> FromRequest<S> for PostForm<T>
where
	T: DeserializeOwned,
	S: Send + Sync,
{
	type Rejection = WebError;

	async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
		if req.method() == Method::POST {
			let axum::Form(data) = axum::Form::from_request(req, state).await?;

			Ok(Self(Some(data)))
		} else {
			Ok(Self(None))
		}
	}
}

/// An extractor which wraps another extractor and converts its errors into
/// `WebError`s.
pub(crate) struct Expect<E>(pub E);

impl<E, S, R> FromRequestParts<S> for Expect<E>
where
	E: FromRequestParts<S, Rejection = R>,
	WebError: From<R>,
	S: Send + Sync,
{
	type Rejection = WebError;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		Ok(Self(E::from_request_parts(parts, state).await?))
	}
}
