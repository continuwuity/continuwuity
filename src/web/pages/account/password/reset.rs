use axum::{
	Extension, Router,
	extract::{Query, State},
	routing::on,
};
use conduwuit_core::warn;
use conduwuit_service::{
	mailer::messages, threepid::session::ValidationSessions, users::HashedPassword,
};
use lettre::{Address, message::Mailbox};
use ruma::{ClientSecret, OwnedClientSecret, OwnedSessionId, UserId};
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError, ValidationErrors};

use crate::{
	WebError,
	extract::{Expect, PostForm},
	form,
	pages::{
		GET_POST, Result, TemplateContext,
		account::ThreepidQuery,
		components::{UserCard, form::Form},
	},
	response,
	session::require_active,
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/", on(GET_POST, route_reset_password))
		.route("/validate", on(GET_POST, route_reset_password_validate))
}

template! {
	struct ResetPassword use "reset_password.html.j2" {
		body: ResetPasswordBody
	}
}

#[derive(Debug)]
enum ResetPasswordBody {
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

async fn route_reset_password(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	PostForm(form): PostForm<ResetPasswordRequestForm>,
) -> Result {
	// Check if SMTP is configured
	if services.mailer.mailer().is_none() {
		return response!(ResetPassword::new(context, ResetPasswordBody::Unavailable));
	}

	// Check if OIDC is enabled
	if services.oidc.enabled() {
		return Err(WebError::BadRequest(
			"Password resets are not available on this server".to_owned(),
		));
	}

	let Some(form) = form else {
		// For GET requests return the reset request form
		return response!(ResetPassword::new(
			context.clone(),
			ResetPasswordBody::Form(ResetPasswordRequestForm::build(context))
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

	response!(ResetPasswordValidate::new(
		context,
		ResetPasswordValidateBody::ValidationPending {
			client_secret,
			session_id,
			validation_error: false
		}
	))
}

template! {
	struct ResetPasswordValidate use "reset_password_validate.html.j2" {
		body: ResetPasswordValidateBody
	}
}

#[derive(Debug)]
enum ResetPasswordValidateBody {
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

async fn route_reset_password_validate(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
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

			if let Err(response) = require_active(&services, &user_id, true).await {
				return Ok(response);
			}

			let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

			if let Some(form) = form {
				if let Err(err) = form.validate() {
					ResetPasswordValidateBody::ValidationSuccess {
						user_card,
						form: ResetPasswordForm::with_errors(context.clone(), err),
					}
				} else {
					match HashedPassword::new(&form.new_password) {
						| Ok(hash) => {
							let _ = session.consume();

							services.users.set_password(&user_id, hash).await?;

							ResetPasswordValidateBody::ResetSuccess { user_card }
						},
						| Err(err) => {
							let mut errors = ValidationErrors::new();

							errors.add(
								"new_password",
								ValidationError::new("malformed")
									.with_message(err.message().into()),
							);

							ResetPasswordValidateBody::ValidationSuccess {
								user_card,
								form: ResetPasswordForm::with_errors(context.clone(), errors),
							}
						},
					}
				}
			} else {
				ResetPasswordValidateBody::ValidationSuccess {
					user_card,
					form: ResetPasswordForm::build(context.clone()),
				}
			}
		},
		| Err(_) => ResetPasswordValidateBody::ValidationPending {
			session_id: query.threepid.session_id,
			client_secret: query.threepid.client_secret,
			validation_error: true,
		},
	};

	response!(ResetPasswordValidate::new(context, body))
}
