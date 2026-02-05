use bytes::BytesMut;
use http::StatusCode;
use http_body_util::Full;
use ruma::api::{
	OutgoingResponse,
	client::{
		error::{ErrorBody, ErrorKind},
		uiaa::UiaaResponse,
	},
};

use super::Error;
use crate::error;

impl axum::response::IntoResponse for Error {
	fn into_response(self) -> axum::response::Response {
		let status = self.status_code();
		if status.is_server_error() {
			error!(
				error = %self,
				error_debug = ?self,
				kind = ?self.kind(),
				status = %status,
				"Server error"
			);
		} else if status.is_client_error() {
			use crate::debug_error;
			debug_error!(
				error = %self,
				kind = ?self.kind(),
				status = %status,
				"Client error"
			);
		}

		let response: UiaaResponse = self.into();
		response
			.try_into_http_response::<BytesMut>()
			.inspect_err(|e| error!("error response error: {e}"))
			.map_or_else(
				|_| StatusCode::INTERNAL_SERVER_ERROR.into_response(),
				|r| r.map(BytesMut::freeze).map(Full::new).into_response(),
			)
	}
}

impl From<Error> for UiaaResponse {
	#[inline]
	fn from(error: Error) -> Self {
		if let Error::Uiaa(uiaainfo) = error {
			return Self::AuthResponse(uiaainfo);
		}

		let body = ErrorBody::Standard {
			kind: error.kind(),
			message: error.message(),
		};

		Self::MatrixError(ruma::api::client::error::Error {
			status_code: error.status_code(),
			body,
		})
	}
}

pub(super) fn status_code(kind: &ErrorKind, hint: StatusCode) -> StatusCode {
	if hint == StatusCode::BAD_REQUEST {
		bad_request_code(kind)
	} else {
		hint
	}
}

pub(super) fn bad_request_code(kind: &ErrorKind) -> StatusCode {
	use ErrorKind::*;

	match kind {
		// 429
		| LimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,

		// 413
		| TooLarge => StatusCode::PAYLOAD_TOO_LARGE,

		// 405
		| Unrecognized => StatusCode::METHOD_NOT_ALLOWED,

		// 404
		| NotFound | NotImplemented | FeatureDisabled => StatusCode::NOT_FOUND,

		// 403
		| GuestAccessForbidden
		| ThreepidAuthFailed
		| UserDeactivated
		| ThreepidDenied
		| InviteBlocked
		| WrongRoomKeysVersion { .. }
		| UserSuspended
		| Forbidden { .. } => StatusCode::FORBIDDEN,

		// 401
		| UnknownToken { .. } | MissingToken | Unauthorized | UserLocked =>
			StatusCode::UNAUTHORIZED,

		// 400
		| _ => StatusCode::BAD_REQUEST,
	}
}

pub(super) fn ruma_error_message(error: &ruma::api::client::error::Error) -> String {
	if let ErrorBody::Standard { message, .. } = &error.body {
		return message.clone();
	}

	format!("{error}")
}

pub(super) fn ruma_error_kind(e: &ruma::api::client::error::Error) -> &ErrorKind {
	e.error_kind().unwrap_or(&ErrorKind::Unknown)
}

pub(super) fn io_error_code(kind: std::io::ErrorKind) -> StatusCode {
	use std::io::ErrorKind;

	match kind {
		| ErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
		| ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
		| ErrorKind::NotFound => StatusCode::NOT_FOUND,
		| ErrorKind::TimedOut => StatusCode::GATEWAY_TIMEOUT,
		| ErrorKind::FileTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
		| ErrorKind::StorageFull => StatusCode::INSUFFICIENT_STORAGE,
		| ErrorKind::Interrupted => StatusCode::SERVICE_UNAVAILABLE,
		| _ => StatusCode::INTERNAL_SERVER_ERROR,
	}
}
