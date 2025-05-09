use super::OidcRequest;
use axum::{
    http::{header::InvalidHeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use oxide_auth::frontends::{dev::OAuthError, simple::endpoint::Error};

#[derive(Debug)]
/// The error type for Oxide Auth operations
pub enum OidcError {
    /// Errors occuring in Endpoint operations
    Endpoint(OAuthError),
    /// Errors occuring in Endpoint operations
    Header(InvalidHeaderValue),
    /// Errors with the request encoding
    Encoding,
    /// Request body could not be parsed as a form
    Form,
    /// Request query was absent or could not be parsed
    Query,
    /// Request query was absent or could not be parsed
    Body,
    /// The Authorization header was invalid
    Authorization,
    /// General internal server error
    InternalError(Option<String>),
}

impl std::fmt::Display for OidcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            OidcError::Endpoint(ref e) => write!(f, "Endpoint, {}", e),
            OidcError::Header(ref e) => write!(f, "Couldn't set header, {}", e),
            OidcError::Encoding => write!(f, "Error decoding request"),
            OidcError::Form => write!(f, "Request is not a form"),
            OidcError::Query => write!(f, "No query present"),
            OidcError::Body => write!(f, "No body present"),
            OidcError::Authorization => write!(f, "Request has invalid Authorization headers"),
            OidcError::InternalError(None) => write!(f, "An internal server error occured"),
            OidcError::InternalError(Some(ref e)) => write!(f, "An internal server error occured: {}", e),
        }
    }
}

impl std::error::Error for OidcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match *self {
            OidcError::Endpoint(ref e) => e.source(),
            OidcError::Header(ref e) => e.source(),
            _ => None,
        }
    }
}

impl IntoResponse for OidcError {
    fn into_response(self) -> Response {
        (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response()
    }
}

impl From<Error<OidcRequest>> for OidcError {
    fn from(e: Error<OidcRequest>) -> Self {
        match e {
            Error::Web(e) => e,
            Error::OAuth(e) => e.into(),
        }
    }
}

impl From<OAuthError> for OidcError {
    fn from(e: OAuthError) -> Self {
        OidcError::Endpoint(e)
    }
}

impl From<InvalidHeaderValue> for OidcError {
    fn from(e: InvalidHeaderValue) -> Self {
        Self::Header(e)
    }
}
