use askama::Template;
use axum::{
	Router,
	extract::{
		Query, State,
		rejection::{FormRejection, QueryRejection},
	},
	http::StatusCode,
	response::{Html, IntoResponse, Response},
	routing::get,
};
use serde::Deserialize;
use validator::Validate;

use crate::{
	WebError, form,
	pages::components::{UserCard, form::Form},
};

#[derive(Deserialize)]
struct PasswordResetQuery {
	token: String,
}

#[derive(Debug, Template)]
#[template(path = "password_reset.html.j2")]
struct PasswordReset<'a> {
	user_card: UserCard<'a>,
	body: PasswordResetBody,
	allow_indexing: bool,
}

#[derive(Debug)]
enum PasswordResetBody {
	Form(Form<'static>),
	Success,
}

form! {
	struct PasswordResetForm {
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

		submit: "Reset Password"
	}
}

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/account/reset_password", get(get_password_reset).post(post_password_reset))
}

async fn password_reset_form(
	services: crate::State,
	query: PasswordResetQuery,
	reset_form: Form<'static>,
) -> Result<impl IntoResponse, WebError> {
	let Some(token) = services.password_reset.check_token(&query.token).await else {
		return Err(WebError::BadRequest("Invalid reset token".to_owned()));
	};

	let user_card = UserCard::for_local_user(&services, &token.info.user).await;

	let template = PasswordReset {
		user_card,
		body: PasswordResetBody::Form(reset_form),
		allow_indexing: services.config.index_page_allow_indexing,
	};

	Ok(Html(template.render()?))
}

async fn get_password_reset(
	State(services): State<crate::State>,
	query: Result<Query<PasswordResetQuery>, QueryRejection>,
) -> Result<impl IntoResponse, WebError> {
	let Query(query) = query?;

	password_reset_form(services, query, PasswordResetForm::build(None)).await
}

async fn post_password_reset(
	State(services): State<crate::State>,
	query: Result<Query<PasswordResetQuery>, QueryRejection>,
	form: Result<axum::Form<PasswordResetForm>, FormRejection>,
) -> Result<Response, WebError> {
	let Query(query) = query?;
	let axum::Form(form) = form?;

	match form.validate() {
		| Ok(()) => {
			let Some(token) = services.password_reset.check_token(&query.token).await else {
				return Err(WebError::BadRequest("Invalid reset token".to_owned()));
			};
			let user_id = token.info.user.clone();

			services
				.password_reset
				.consume_token(token, &form.new_password)
				.await?;

			let user_card = UserCard::for_local_user(&services, &user_id).await;
			let template = PasswordReset {
				user_card,
				body: PasswordResetBody::Success,
				allow_indexing: services.config.index_page_allow_indexing,
			};

			Ok(Html(template.render()?).into_response())
		},
		| Err(err) => Ok((
			StatusCode::BAD_REQUEST,
			password_reset_form(services, query, PasswordResetForm::build(Some(err))).await,
		)
			.into_response()),
	}
}
