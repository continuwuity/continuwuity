use axum::{
	Router,
	extract::{Query, State},
	routing::{get, on, post},
};
use conduwuit_core::warn;
use conduwuit_service::{mailer::messages, threepid::session::ValidationSessions};
use lettre::{Address, message::Mailbox};
use ruma::{ClientSecret, OwnedClientSecret, OwnedSessionId};
use serde::{Deserialize, Serialize};

use crate::{
	WebError,
	extract::{Expect, PostForm},
	form,
	pages::{
		GET_POST, Result,
		account::ThreepidQuery,
		components::{UserCard, form::Form},
	},
	response,
	session::{LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/change/", on(GET_POST, route_change_email_request))
		.route("/change/validate", get(get_change_email))
		.route("/change/delete", post(post_delete_email))
}

template! {
	struct ChangeEmailRequest use "change_email_request.html.j2" {
		user_card: UserCard,
		email: Option<String>,
		form: Form<'static>,
		may_remove: bool
	}
}

form! {
	struct ChangeEmailRequestForm {
		email: Address where {
			input_type: "email",
			label: "Email address"
		}

		submit: "Change email"
	}
}

template! {
	struct ChangeEmail use "change_email.html.j2" {
		user_card: UserCard,
		body: ChangeEmailBody
	}
}

template! {
	struct DeleteEmail use "delete_email.html.j2" {
		user_card: UserCard
	}
}

#[derive(Debug)]
enum ChangeEmailBody {
	ValidationPending {
		session_id: OwnedSessionId,
		client_secret: OwnedClientSecret,
		validation_error: bool,
	},
	Success,
}

async fn route_change_email_request(
	State(services): State<crate::State>,
	user: User,
	PostForm(form): PostForm<ChangeEmailRequestForm>,
) -> Result {
	let user_id = user.expect_recent(LoginTarget::ChangeEmail)?;

	let template = ChangeEmailRequest::new(
		&services,
		UserCard::for_local_user(&services, user_id.clone()).await,
		services
			.threepid
			.get_email_for_localpart(user_id.localpart())
			.await
			.map(|address| address.to_string()),
		ChangeEmailRequestForm::build(),
		services.threepid.email_requirement().may_remove(),
	);

	let Some(form) = form else {
		return response!(template);
	};

	let client_secret = ClientSecret::new();

	let session_id = {
		let display_name = services.users.displayname(&user_id).await.ok();

		match services
			.threepid
			.send_validation_email(
				Mailbox::new(display_name, form.email.clone()),
				|verification_link| messages::ChangeEmail {
					server_name: services.globals.server_name().as_str(),
					user_id: Some(&user_id),
					verification_link,
				},
				&client_secret,
				0,
			)
			.await
		{
			| Ok(session_id) => session_id,
			| Err(err) => {
				// If we couldn't send an email, generate a random session ID to not give that
				// away
				warn!(
					"Failed to send email change message for {user_id} to {}: {err}",
					form.email
				);

				ValidationSessions::generate_session_id()
			},
		}
	};

	response!(ChangeEmail::new(
		&services,
		UserCard::for_local_user(&services, user_id).await,
		ChangeEmailBody::ValidationPending {
			session_id,
			client_secret,
			validation_error: false
		}
	))
}

#[derive(Deserialize, Serialize)]
struct ChangeEmailQuery {
	#[serde(flatten)]
	threepid: ThreepidQuery,
}

async fn get_change_email(
	State(services): State<crate::State>,
	Expect(Query(ChangeEmailQuery {
		threepid: ThreepidQuery { client_secret, session_id },
	})): Expect<Query<ChangeEmailQuery>>,
	user: User,
) -> Result {
	let user_id = user.expect(LoginTarget::ChangeEmail)?;
	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

	if !services.threepid.email_requirement().may_change() {
		return Err(WebError::Forbidden("You may not change your email address.".to_owned()));
	}

	let Ok(session) = services
		.threepid
		.get_valid_session(&session_id, &client_secret)
		.await
	else {
		return response!(ChangeEmail::new(
			&services,
			user_card,
			ChangeEmailBody::ValidationPending {
				session_id,
				client_secret,
				validation_error: true
			}
		));
	};

	let new_email = session.consume();

	if let Err(err) = services
		.threepid
		.associate_localpart_email(user_id.localpart(), &new_email)
		.await
	{
		return response!(BadRequest(err.message()));
	}

	response!(ChangeEmail::new(&services, user_card, ChangeEmailBody::Success))
}

async fn post_delete_email(State(services): State<crate::State>, user: User) -> Result {
	let user_id = user.expect(LoginTarget::ChangeEmail)?;
	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

	if !services.threepid.email_requirement().may_remove() {
		return Err(WebError::Forbidden("You may not remove your email address.".to_owned()));
	}

	let _ = services
		.threepid
		.disassociate_localpart_email(user_id.localpart())
		.await;

	response!(DeleteEmail::new(&services, user_card))
}
