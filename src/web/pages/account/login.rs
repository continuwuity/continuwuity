use std::time::SystemTime;

use axum::{
	Extension, Router,
	extract::{Query, RawQuery, State},
	response::{IntoResponse, Redirect},
	routing::{get, on},
};
use conduwuit_api::client::handle_login;
use openidconnect::core::CoreAuthPrompt;
use ruma::{
	OwnedUserId,
	api::client::uiaa::{EmailUserIdentifier, MatrixUserIdentifier, UserIdentifier},
};
use serde::Deserialize;
use tower_sessions::Session;
use url::Url;

use crate::{
	ROUTE_PREFIX, WebError,
	extract::{Expect, PostForm},
	pages::{
		GET_POST, Result, TemplateContext,
		account::register::{TrustedFlowStatus, UntrustedFlowStatus, registration_flow_status},
		components::UserCard,
		oidc::{OIDC_SESSION_ID_KEY, OidcSession, OidcSessionState},
	},
	response,
	session::{LoginIntent, LoginQuery, LoginTarget, User, UserSession},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/login", on(GET_POST, route_login))
		.route("/logout", get(get_logout))
}

template! {
	struct Login use "login.html.j2" {
		body: LoginBody,
		login_type: LoginType,
		login_error: Option<String>
	}
}

#[derive(Debug)]
enum LoginBody {
	Unauthenticated {
		server_name: String,
		next: Option<LoginTarget>,
	},
	Authenticated {
		user_card: UserCard,
	},
}

#[derive(Debug)]
enum LoginType {
	Interactive {
		registration_available: bool,
	},
	Oidc {
		redirect_url: Url,
	},
}

#[derive(Deserialize)]
struct LoginForm {
	identifier: Option<String>,
	password: String,
}

async fn route_login(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	Expect(Query(LoginQuery { next, reauthenticate, intent })): Expect<Query<LoginQuery>>,
	session_store: Session,
	user: User<true>,
	PostForm(form): PostForm<LoginForm>,
) -> Result {
	let user_id = user.into_session().map(|session| session.user_id);

	let login_type = if services.oidc.enabled() {
		let (session, redirect_url) = services
			.oidc
			.begin_session(match intent {
				| Some(LoginIntent::SwitchAccounts) => Some(CoreAuthPrompt::SelectAccount),
				| _ if reauthenticate => Some(CoreAuthPrompt::Consent),
				| _ => None,
			})
			.await;

		session_store
			.insert(OIDC_SESSION_ID_KEY, OidcSession {
				next: next.clone().unwrap_or_default(),
				state: OidcSessionState::CodeExchange { expected_user: user_id.clone(), session },
			})
			.await
			.expect("should be able to serialize OIDC session");

		if next.is_some() {
			return response!(Redirect::to(redirect_url.as_str()));
		}

		LoginType::Oidc { redirect_url }
	} else {
		let (trusted_flow_status, untrusted_flow_status) =
			registration_flow_status(&services).await;

		let registration_available = matches!(trusted_flow_status, TrustedFlowStatus::Available)
			|| matches!(untrusted_flow_status, UntrustedFlowStatus::Available { .. });

		LoginType::Interactive { registration_available }
	};

	let body = match &user_id {
		| None => LoginBody::Unauthenticated {
			server_name: services.globals.server_name().to_string(),
			next: next.clone(),
		},
		| Some(user_id) => {
			if !reauthenticate {
				return response!(Redirect::to(&next.unwrap_or_default().target_path()));
			}

			let user_card = UserCard::for_local_user(&services, user_id.to_owned()).await;

			LoginBody::Authenticated { user_card }
		},
	};

	let mut template = Login::new(context, body, login_type, None);

	if let Some(form) = form {
		let login_result = match (user_id, form.identifier) {
			| (Some(user_id), _) => {
				// The user is already authenticated, we need to check their password
				services
					.users
					.check_password(&user_id, &form.password)
					.await
			},
			| (None, Some(identifier)) => {
				// The user isn't authenticated, we need to log them in
				let identifier = if identifier.parse::<lettre::Address>().is_ok() {
					UserIdentifier::Email(EmailUserIdentifier::new(identifier))
				} else {
					UserIdentifier::Matrix(MatrixUserIdentifier::new(identifier))
				};

				handle_login(&services, Some(&identifier), &form.password, None).await
			},
			| (None, None) => {
				// The user isn't authenticated and didn't supply an identity
				return response!(WebError::BadRequest("No identity provided".to_owned()));
			},
		};

		let user_id = match login_result {
			| Ok(user_id) => user_id,
			| Err(err) => {
				let error_message = if let conduwuit_core::Error::Request(_, message, _) = err {
					message.into_owned()
				} else {
					"Internal login error".to_owned()
				};

				template.login_error = Some(error_message);
				return response!(template);
			},
		};

		let user_session = UserSession { user_id, last_login: SystemTime::now() };

		session_store
			.insert(User::KEY, user_session)
			.await
			.expect("should be able to serialize user session");

		return response!(Redirect::to(&next.unwrap_or_default().target_path()));
	}

	response!(template)
}

async fn get_logout(session: Session, RawQuery(query): RawQuery) -> impl IntoResponse {
	let _ = session.remove::<OwnedUserId>(User::KEY).await;

	Redirect::to(&format!("{}/account/login?{}", ROUTE_PREFIX, query.unwrap_or_default()))
}
