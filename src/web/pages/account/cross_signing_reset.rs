use axum::{Router, extract::State, routing::on};
use conduwuit_service::oauth::OAuthTicket;

use crate::{
	extract::PostForm,
	pages::{GET_POST, Result, components::UserCard},
	response,
	session::{LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/", on(GET_POST, route_cross_signing_reset))
}

template! {
	struct CrossSigningReset use "cross_signing_reset.html.j2" {
		user_card: UserCard,
		body: CrossSigningResetBody
	}
}

#[derive(Debug)]
enum CrossSigningResetBody {
	Form,
	Success,
}

async fn route_cross_signing_reset(
	State(services): State<crate::State>,
	user: User,
	PostForm(form): PostForm<()>,
) -> Result {
	let user_id = user.expect_recent(LoginTarget::CrossSigningReset)?;
	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

	if form.is_some() {
		services
			.oauth
			.issue_ticket(user_id.localpart().to_owned(), OAuthTicket::CrossSigningReset);

		response!(CrossSigningReset::new(&services, user_card, CrossSigningResetBody::Success))
	} else {
		response!(CrossSigningReset::new(&services, user_card, CrossSigningResetBody::Form))
	}
}
