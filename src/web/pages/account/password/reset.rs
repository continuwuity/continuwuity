use axum::{
	Router,
	extract::{Query, State},
	routing::on,
};
use conduwuit_core::warn;
use conduwuit_service::{mailer::messages, threepid::session::ValidationSessions};
use lettre::{Address, message::Mailbox};
use ruma::{ClientSecret, OwnedClientSecret, OwnedSessionId, UserId};
use serde::{Deserialize, Serialize};
use validator::Validate;

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
	session::require_active,
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/", on(GET_POST, route_reset_password_request))
		.route("/validate", on(GET_POST, route_reset_password))
}

template! {
	struct ResetPasswordRequest use "reset_password_request.html.j2" {
		body: ResetPasswordRequestBody
	}
}

#[derive(Debug)]
enum ResetPasswordRequestBody {
	Form(Form<'static>),
	Unavailable,
}

form! {
	struct ResetPasswordRequestForm {
		email: Address where {
			input_type: "email",
			label: "Email address"
		}

		submit: "Send email"
	}
}

async fn route_reset_password_request(
	State(services): State<crate::State>,
	PostForm(form): PostForm<ResetPasswordRequestForm>,
) -> Result {
	// Check if SMTP is configured
	if services.mailer.mailer().is_none() {
		return response!(ResetPasswordRequest::new(
			&services,
			ResetPasswordRequestBody::Unavailable
		));
	}

	let Some(form) = form else {
		// For GET requests return the reset request form
		return response!(ResetPasswordRequest::new(
			&services,
			ResetPasswordRequestBody::Form(ResetPasswordRequestForm::build())
		));
	};

	let client_secret = ClientSecret::new();

	let session_id = async {
		let Some(localpart) = services.threepid.get_localpart_for_email(&form.email).await else {
			warn!("No user is associated with the email address {}", form.email);

			return None;
		};

		let user_id =
			UserId::parse(format!("@{localpart}:{}", services.globals.server_name())).unwrap();
		let display_name = services.users.displayname(&user_id).await.ok();

		match services
			.threepid
			.send_validation_email(
				Mailbox::new(display_name.clone(), form.email.clone()),
				|verification_link| messages::PasswordReset {
					display_name: display_name.as_deref(),
					user_id: &user_id,
					verification_link,
				},
				&client_secret,
				0,
			)
			.await
		{
			| Ok(session_id) => Some(session_id),
			| Err(err) => {
				warn!("Failed to send reset email for {localpart} to {}: {err}", form.email);

				None
			},
		}
	}
	.await
	.unwrap_or_else(|| {
		// If we couldn't send an email, generate a random session ID to not give that
		// away
		ValidationSessions::generate_session_id()
	});

	response!(ResetPassword::new(&services, ResetPasswordBody::ValidationPending {
		client_secret,
		session_id,
		validation_error: false
	}))
}

template! {
	struct ResetPassword use "reset_password.html.j2" {
		body: ResetPasswordBody
	}
}

#[derive(Debug)]
enum ResetPasswordBody {
	ValidationPending {
		session_id: OwnedSessionId,
		client_secret: OwnedClientSecret,
		validation_error: bool,
	},
	ValidationSuccess {
		user_card: UserCard,
		form: Form<'static>,
	},
	ResetSuccess {
		user_card: UserCard,
	},
}

form! {
	struct ResetPasswordForm {
		#[validate(length(min = 1, message = "Password cannot be empty"))]
		new_password: String where {
			input_type: "password",
			label: "New password",
			autocomplete: "new-password"
		},

		#[validate(must_match(other = "new_password", message = "Passwords must match"))]
		confirm_new_password: String where {
			input_type: "password",
			label: "Confirm new password",
			autocomplete: "new-password"
		}

		submit: "Reset password"
	}
}

#[derive(Deserialize, Serialize)]
struct ResetPasswordQuery {
	#[serde(flatten)]
	threepid: ThreepidQuery,
}

async fn route_reset_password(
	State(services): State<crate::State>,
	Expect(Query(query)): Expect<Query<ResetPasswordQuery>>,
	PostForm(form): PostForm<ResetPasswordForm>,
) -> Result {
	let body = match services
		.threepid
		.get_valid_session(&query.threepid.session_id, &query.threepid.client_secret)
		.await
	{
		| Ok(session) => {
			let Some(localpart) = services
				.threepid
				.get_localpart_for_email(&session.email)
				.await
			else {
				return Err(WebError::BadRequest("Inapplicable threepid session.".to_owned()));
			};

			let user_id =
				UserId::parse(format!("@{localpart}:{}", services.globals.server_name()))
					.unwrap();

			require_active(&services, &user_id).await?;
			if services
				.users
				.origin(&user_id)
				.await
				.ok()
				.is_some_and(|origin| origin != "password")
			{
				return Err(WebError::BadRequest("You cannot change your password.".to_owned()));
			}

			let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

			if let Some(form) = form {
				if let Err(err) = form.validate() {
					ResetPasswordBody::ValidationSuccess {
						user_card,
						form: ResetPasswordForm::with_errors(err),
					}
				} else {
					services
						.users
						.set_password(&user_id, Some(&form.new_password))
						.await?;
					let _ = session.consume();

					ResetPasswordBody::ResetSuccess { user_card }
				}
			} else {
				ResetPasswordBody::ValidationSuccess {
					user_card,
					form: ResetPasswordForm::build(),
				}
			}
		},
		| Err(_) => ResetPasswordBody::ValidationPending {
			session_id: query.threepid.session_id,
			client_secret: query.threepid.client_secret,
			validation_error: true,
		},
	};

	response!(ResetPassword::new(&services, body))
}
