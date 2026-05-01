use axum::{Router, extract::State, routing::on};
use conduwuit_api::client::{full_user_deactivate, password_login};
use futures::StreamExt;
use ruma::{OwnedRoomId, OwnedUserId, UserId};
use tower_sessions::Session;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::{
	extract::PostForm,
	form,
	pages::{
		GET_POST, Result,
		components::{UserCard, form::Form},
	},
	response,
	session::{LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/", on(GET_POST, route_deactivate))
}

template! {
	struct Deactivate use "deactivate.html.j2" {
		body: DeactivateBody
	}
}

#[derive(Debug)]
enum DeactivateBody {
	Form {
		user_id: OwnedUserId,
		user_card: UserCard,
		form: Form<'static>,
	},
	Success,
}

form! {
	struct DeactivateForm {
		password: String where {
			input_type: "password",
			label: "Enter your password to confirm",
			autocomplete: "current-password"
		},
		#[validate(required(message = "This checkbox must be checked"))]
		confirm: Option<String> where {
			input_type: "checkbox",
			label: "I understand that deactivating my account cannot be undone."
		}

		submit: "Deactivate my account",
		slowdown: true
	}
}

async fn route_deactivate(
	State(services): State<crate::State>,
	user: User,
	session: Session,
	PostForm(form): PostForm<DeactivateForm>,
) -> Result {
	let user_id = user.expect_recent(LoginTarget::Deactivate)?;
	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

	let body = {
		if let Some(form) = form {
			if let Err(err) = validate_deactivate_form(&services, &user_id, form).await {
				DeactivateBody::Form {
					user_id,
					user_card,
					form: DeactivateForm::with_errors(err),
				}
			} else {
				let all_joined_rooms: Vec<OwnedRoomId> = services
					.rooms
					.state_cache
					.rooms_joined(&user_id)
					.collect()
					.await;

				full_user_deactivate(&services, &user_id, &all_joined_rooms).await?;

				session.clear().await;

				DeactivateBody::Success
			}
		} else {
			DeactivateBody::Form {
				user_id,
				user_card,
				form: DeactivateForm::build(),
			}
		}
	};

	response!(Deactivate::new(&services, body))
}

async fn validate_deactivate_form(
	services: &crate::State,
	user_id: &UserId,
	form: DeactivateForm,
) -> Result<(), ValidationErrors> {
	form.validate()?;

	if password_login(services, user_id, user_id, &form.password)
		.await
		.is_err()
	{
		let mut errors = ValidationErrors::new();
		errors.add(
			"password",
			ValidationError::new("wrong").with_message("Incorrect password".into()),
		);

		return Err(errors);
	}

	Ok(())
}
