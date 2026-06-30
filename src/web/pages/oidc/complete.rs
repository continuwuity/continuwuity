use std::time::SystemTime;

use axum::{
	Extension, Router,
	extract::{Query, State},
	response::Redirect,
	routing::on,
};
use conduwuit_service::{oauth::grant::AuthorizationCodeResponse, oidc::SessionCompletionStatus};
use futures::FutureExt;
use ruma::OwnedServerName;
use serde::{Deserialize, de::IgnoredAny};
use tower_sessions::Session;

use crate::{
	WebError,
	extract::{Expect, PostForm},
	pages::{
		GET_POST, Result, TemplateContext,
		oidc::{OIDC_SESSION_ID_KEY, OidcSession, OidcSessionState},
	},
	response,
	session::{User, UserSession},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/", on(GET_POST, route_complete))
}

template! {
	struct OidcComplete use "oidc_complete.html.j2" {
		server_name: OwnedServerName,
		username_error: Option<String>
	}
}

#[derive(Deserialize)]
struct LoginForm {
	username: String,
}

async fn route_complete(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	Expect(Query(query)): Expect<Query<AuthorizationCodeResponse>>,
	session_store: Session,
	user: User<true>,
	PostForm(form): PostForm<LoginForm>,
) -> Result {
	let user_id = user.into_session().map(|session| session.user_id);

	let Some(session) = session_store
		.get::<OidcSession>(OIDC_SESSION_ID_KEY)
		.await
		.expect("should be able to deserialize oidc session")
	else {
		return response!(WebError::BadRequest(
			"No OIDC session found. What are you doing here?".to_owned()
		));
	};

	let session_completion_status = match session.state {
		| OidcSessionState::CodeExchange { expected_user, session: pending_session } => {
			if let (Some(user_id), Some(expected_user)) = (&user_id, &expected_user)
				&& user_id != expected_user
			{
				return response!(WebError::BadRequest(
					"Identity mismatch. You may have switched accounts at your identity \
					 provider. Please log out and back in to continue."
						.to_owned()
				));
			}

			let claims = services
				.oidc
				.exchange_code(pending_session, query)
				.boxed()
				.await
				.map_err(|err| WebError::BadRequest(err.to_owned()))?;

			session_store
				.insert(OIDC_SESSION_ID_KEY, OidcSession {
					next: session.next.clone(),
					state: OidcSessionState::Authorized { claims: Box::new(claims.clone()) },
				})
				.await
				.expect("Should be able to serialize oidc session");

			services.oidc.complete_session(&claims, None).await
		},
		| OidcSessionState::Authorized { claims } =>
			services
				.oidc
				.complete_session(&claims, form.map(|form| form.username))
				.await,
	}
	.map_err(|err| WebError::BadRequest(err.to_owned()))?;

	match session_completion_status {
		| SessionCompletionStatus::Complete(user_id) => {
			let _ = session_store
				.remove::<IgnoredAny>(OIDC_SESSION_ID_KEY)
				.await;

			let user_session = UserSession { user_id, last_login: SystemTime::now() };

			session_store
				.insert(User::KEY, user_session)
				.await
				.expect("should be able to serialize user session");

			response!(Redirect::to(&session.next.target_path()))
		},
		| SessionCompletionStatus::NeedsLocalpart => {
			response!(OidcComplete::new(context, services.globals.server_name().to_owned(), None))
		},
		| SessionCompletionStatus::InvalidLocalpart(error) => {
			response!(OidcComplete::new(
				context,
				services.globals.server_name().to_owned(),
				Some(error)
			))
		},
	}
}
