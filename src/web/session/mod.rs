use std::time::{Duration, SystemTime};

use axum::{extract::FromRequestParts, http::request::Parts};
use ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::{ROUTE_PREFIX, WebError};

pub(crate) mod store;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LoginQuery {
	#[serde(flatten)]
	pub next: LoginTarget,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub reauthenticate: bool,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(tag = "next", rename_all = "snake_case")]
pub(crate) enum LoginTarget {
	#[default]
	Account,
	ChangePassword,
	ChangeEmail,
	CrossSigningReset,
	Deactivate,
}

impl LoginTarget {
	pub(crate) fn target_path(&self) -> String {
		let path = match self {
			| Self::Account => "account/",
			| Self::ChangePassword => "account/password/change",
			| Self::ChangeEmail => "account/email/change/",
			| Self::CrossSigningReset => "account/cross_signing_reset",
			| Self::Deactivate => "account/deactivate",
		};

		format!("{ROUTE_PREFIX}/{path}")
	}
}

/// An extractor that fetches the authenticated user.
pub(crate) struct User(Option<UserSession>);

#[derive(Serialize, Deserialize)]
pub(crate) struct UserSession {
	pub user_id: OwnedUserId,
	pub last_login: SystemTime,
}

impl UserSession {
	const RECENT_LOGIN_THRESHOLD: Duration = Duration::from_mins(10);

	pub(crate) fn is_recent(&self) -> bool {
		let now = SystemTime::now();

		if let Ok(duration) = now.duration_since(self.last_login) {
			duration < Self::RECENT_LOGIN_THRESHOLD
		} else {
			// Clock drift might cause the last login time to be later than the current
			// system time. We play it safe and say the session isn't recent if that
			// happens.
			false
		}
	}
}

impl User {
	pub(crate) const KEY: &str = "session";

	/// Consume this extractor and return the user's session information.
	pub(crate) fn into_session(self) -> Option<UserSession> { self.0 }

	/// Extract the user ID, redirecting to the login page if the user isn't
	/// logged in.
	pub(crate) fn expect(self, or_else: LoginTarget) -> Result<OwnedUserId, WebError> {
		if let Some(session) = self.0 {
			Ok(session.user_id)
		} else {
			Err(WebError::LoginRequired(LoginQuery { next: or_else, reauthenticate: false }))
		}
	}

	/// Extract the user ID, redirecting to the login page if the user isn't
	/// logged in or if they haven't logged in recently.
	pub(crate) fn expect_recent(self, or_else: LoginTarget) -> Result<OwnedUserId, WebError> {
		if let Some(session) = self.0 {
			if session.is_recent() {
				Ok(session.user_id)
			} else {
				Err(WebError::LoginRequired(LoginQuery { next: or_else, reauthenticate: true }))
			}
		} else {
			Err(WebError::LoginRequired(LoginQuery { next: or_else, reauthenticate: false }))
		}
	}
}

impl FromRequestParts<crate::State> for User {
	type Rejection = WebError;

	async fn from_request_parts(
		parts: &mut Parts,
		services: &crate::State,
	) -> Result<Self, Self::Rejection> {
		let session_store = Session::from_request_parts(parts, services)
			.await
			.expect("should be able to extract session");

		let session = session_store
			.get::<UserSession>(Self::KEY)
			.await
			.expect("should be able to deserialize session");

		if let Some(session) = &session {
			require_active(services, &session.user_id).await?;
		}

		Ok(Self(session))
	}
}

pub(crate) async fn require_active(
	services: &crate::State,
	user_id: &UserId,
) -> Result<(), WebError> {
	if !services.users.is_active(user_id).await {
		return Err(WebError::Forbidden("Your account is deactivated.".to_owned()));
	}

	if services
		.users
		.is_locked(user_id)
		.await
		.expect("should be able to check lock state")
	{
		return Err(WebError::Forbidden("Your account is locked.".to_owned()));
	}

	Ok(())
}
