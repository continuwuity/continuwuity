use std::{
	borrow::Cow,
	mem::discriminant,
	time::{Duration, SystemTime},
};

use axum::{
	extract::FromRequestParts,
	http::request::Parts,
	response::{IntoResponse, Redirect, Response},
};
use conduwuit_service::oauth::grant::{AuthorizationCodeQuery, DeviceCodeVerifyQuery};
use ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;

use crate::{ROUTE_PREFIX, WebError, pages::account::device::DevicePath};

pub(crate) mod store;

#[derive(Default, Debug, Deserialize, Serialize)]
pub(crate) struct LoginQuery {
	#[serde(flatten)]
	pub next: Option<LoginTarget>,
	#[serde(default, skip_serializing_if = "std::ops::Not::not")]
	pub reauthenticate: bool,
	#[serde(default)]
	pub intent: Option<LoginIntent>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(tag = "next", rename_all = "snake_case")]
pub(crate) enum LoginTarget {
	AuthorizationCode(AuthorizationCodeQuery),
	#[default]
	Account,
	ChangePassword,
	ChangeEmail,
	CrossSigningReset,
	Deactivate,
	DeviceCode(DeviceCodeVerifyQuery),
	DeviceInfo(DevicePath),
	RemoveDevice(DevicePath),
}

impl PartialEq for LoginTarget {
	fn eq(&self, other: &Self) -> bool { discriminant(self) == discriminant(other) }
}

impl LoginTarget {
	pub(crate) fn target_path(&self) -> String {
		let path: Cow<'_, str> = match self {
			| Self::AuthorizationCode(code) => format!(
				"oauth2/grant/authorization_code?{}",
				serde_urlencoded::to_string(code).unwrap()
			)
			.into(),
			| Self::Account => "account/".into(),
			| Self::ChangePassword => "account/password/change".into(),
			| Self::ChangeEmail => "account/email/change/".into(),
			| Self::CrossSigningReset => "account/cross_signing_reset".into(),
			| Self::Deactivate => "account/deactivate".into(),
			| Self::DeviceCode(code) =>
				format!("oauth2/grant/device_code?{}", serde_urlencoded::to_string(code).unwrap())
					.into(),
			| Self::DeviceInfo(path) => format!("account/device/{}/", path.device).into(),
			| Self::RemoveDevice(path) => format!("account/device/{}/remove", path.device).into(),
		};

		format!("{ROUTE_PREFIX}/{path}")
	}
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LoginIntent {
	SwitchAccounts,
}

/// An extractor that fetches the authenticated user.
pub(crate) struct User<const ALLOW_LOCKED: bool = false>(Option<UserSession>);

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
}

impl<const ALLOW_LOCKED: bool> User<ALLOW_LOCKED> {
	/// Consume this extractor and return the user's session information.
	pub(crate) fn into_session(self) -> Option<UserSession> { self.0 }

	/// Extract the user ID, redirecting to the login page if the user isn't
	/// logged in.
	pub(crate) fn expect(self, or_else: LoginTarget) -> Result<OwnedUserId, WebError> {
		if let Some(session) = self.0 {
			Ok(session.user_id)
		} else {
			Err(WebError::LoginRequired(LoginQuery {
				next: Some(or_else),
				..Default::default()
			}))
		}
	}

	/// Extract the user ID, redirecting to the login page if the user isn't
	/// logged in or if they haven't logged in recently.
	pub(crate) fn expect_recent(self, or_else: LoginTarget) -> Result<OwnedUserId, WebError> {
		if let Some(session) = self.0 {
			if session.is_recent() {
				Ok(session.user_id)
			} else {
				Err(WebError::LoginRequired(LoginQuery {
					next: Some(or_else),
					reauthenticate: true,
					..Default::default()
				}))
			}
		} else {
			Err(WebError::LoginRequired(LoginQuery {
				next: Some(or_else),
				..Default::default()
			}))
		}
	}
}

impl<const ALLOW_LOCKED: bool> FromRequestParts<crate::State> for User<ALLOW_LOCKED> {
	type Rejection = Response;

	async fn from_request_parts(
		parts: &mut Parts,
		services: &crate::State,
	) -> Result<Self, Self::Rejection> {
		let session_store = Session::from_request_parts(parts, services)
			.await
			.expect("should be able to extract session");

		let session = session_store
			.get::<UserSession>(User::KEY)
			.await
			.expect("should be able to deserialize session");

		if let Some(session) = &session {
			require_active(services, &session.user_id, ALLOW_LOCKED).await?;
		}

		Ok(Self(session))
	}
}

pub(crate) async fn require_active(
	services: &crate::State,
	user_id: &UserId,
	allow_locked: bool,
) -> Result<(), Response> {
	if let Err(err) = services.users.status(user_id).await.ensure_active() {
		return Err(WebError::Forbidden(err.message()).into_response());
	}

	if !allow_locked
		&& services
			.users
			.is_locked(user_id)
			.await
			.expect("should be able to check lock state")
	{
		return Err(Redirect::to(&format!("{ROUTE_PREFIX}/account/")).into_response());
	}

	Ok(())
}
