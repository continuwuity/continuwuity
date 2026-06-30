use axum::Router;
use conduwuit_service::oidc;
use openidconnect::core::CoreIdTokenClaims;
use ruma::OwnedUserId;
use serde::{Deserialize, Serialize};

use crate::session::LoginTarget;

mod complete;

pub(crate) const OIDC_SESSION_ID_KEY: &str = "oidc_session";

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct OidcSession {
	pub next: LoginTarget,
	pub state: OidcSessionState,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) enum OidcSessionState {
	CodeExchange {
		expected_user: Option<OwnedUserId>,
		session: oidc::PendingSession,
	},
	Authorized {
		claims: Box<CoreIdTokenClaims>,
	},
}

pub(crate) fn build() -> Router<crate::State> {
	#[allow(clippy::wildcard_imports)]
	use self::*;

	Router::new().nest("/complete", complete::build())
}
