use std::time::SystemTime;

use axum::{
	Router,
	extract::{Query, State},
	response::{IntoResponse, Redirect},
	routing::{get, on},
};
use conduwuit_api::client::{handle_login, password_login};
use ruma::{
	OwnedUserId,
	api::client::uiaa::{EmailUserIdentifier, MatrixUserIdentifier, UserIdentifier},
};
use serde::{Deserialize, Serialize};
use tower_sessions::Session;
use validator::Validate;

use crate::{
	WebError,
	extract::{Expect, PostForm},
	pages::{GET_POST, Result, components::UserCard},
	response,
	session::{LoginQuery, User, UserSession},
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
		login_error: Option<String>
	}
}

#[derive(Debug)]
enum LoginBody {
	Unauthenticated {
		server_name: String,
	},
	Authenticated {
		user_card: UserCard,
	},
}

#[derive(Deserialize)]
struct LoginForm {
	identifier: Option<String>,
	password: String,
}

async fn route_login(
	State(services): State<crate::State>,
	Expect(Query(query)): Expect<Query<LoginQuery>>,
	session_store: Session,
	user: User,
	PostForm(form): PostForm<LoginForm>,
) -> Result {
	let user_id = user.into_session().map(|session| session.user_id);

	let body = match &user_id {
		| None => LoginBody::Unauthenticated {
			server_name: services.globals.server_name().to_string(),
		},
		| Some(user_id) => {
			if !query.reauthenticate {
				return response!(Redirect::to(&query.next.target_path()));
			}

			let user_card = UserCard::for_local_user(&services, user_id.to_owned()).await;

			LoginBody::Authenticated { user_card }
		},
	};

	let mut template = Login::new(&services, body, None);

	if let Some(form) = form {
		let login_result = match (user_id, form.identifier) {
			| (Some(user_id), _) => {
				// The user is already authenticated, we need to check their password
				password_login(&services, &user_id, &user_id, &form.password).await
			},
			| (None, Some(identifier)) => {
				// The user isn't authenticated, we need to log them in
				// Yes, this does parse the email twice (handle_login does it again). I don't
				// think this really needs to be optimized.
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

		return response!(Redirect::to(&query.next.target_path()));
	}

	response!(template)
}

async fn get_logout(session: Session) -> impl IntoResponse {
	let _ = session.remove::<OwnedUserId>(User::KEY).await;

	Redirect::to("/_continuwuity/account/")
}
