use axum::{Extension, Router, extract::State, routing::on};
use conduwuit_service::users::HashedPassword;
use ruma::UserId;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::{
	WebError,
	extract::PostForm,
	form,
	pages::{
		GET_POST, Result, TemplateContext,
		components::{UserCard, form::Form},
	},
	response,
	session::{LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/", on(GET_POST, route_change_password))
}

template! {
	struct ChangePassword use "change_password.html.j2" {
		user_card: UserCard,
		body: ChangePasswordBody
	}
}

#[derive(Debug)]
enum ChangePasswordBody {
	Form(Form<'static>),
	Success,
}

form! {
	struct ChangePasswordForm {
		#[validate(length(min = 1, message = "Current password cannot be empty"))]
		current_password: String where {
			input_type: "password",
			label: "Current password",
			autocomplete: "current-password"
		},

		#[validate(length(min = 1, message = "New password cannot be empty"))]
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

		submit: "Change password"
	}
}

async fn route_change_password(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	user: User,
	PostForm(form): PostForm<ChangePasswordForm>,
) -> Result {
	if services.oidc.enabled() {
		return Err(WebError::BadRequest(
			"Password changing is not available on this server".to_owned(),
		));
	}

	let user_id = user.expect(LoginTarget::ChangePassword)?;
	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

	let body = if let Some(form) = form {
		match change_password(&services, &user_id, form).await? {
			| Ok(()) => ChangePasswordBody::Success,
			| Err(errors) =>
				ChangePasswordBody::Form(ChangePasswordForm::with_errors(context.clone(), errors)),
		}
	} else {
		ChangePasswordBody::Form(ChangePasswordForm::build(context.clone()))
	};

	response!(ChangePassword::new(context, user_card, body))
}

async fn change_password(
	services: &crate::State,
	user_id: &UserId,
	form: ChangePasswordForm,
) -> Result<Result<(), ValidationErrors>> {
	form.validate()?;

	if services
		.users
		.check_password(user_id, &form.current_password)
		.await
		.is_err()
	{
		let mut errors = ValidationErrors::new();
		errors.add(
			"current_password",
			ValidationError::new("wrong").with_message("Incorrect password".into()),
		);

		return Ok(Err(errors));
	}

	match HashedPassword::new(&form.new_password) {
		| Ok(hash) => {
			services.users.set_password(user_id, hash).await?;
		},
		| Err(err) => {
			let mut errors = ValidationErrors::new();
			errors.add(
				"new_password",
				ValidationError::new("malformed").with_message(err.message().into()),
			);

			return Ok(Err(errors));
		},
	}

	Ok(Ok(()))
}
