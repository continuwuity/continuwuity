use std::collections::BTreeSet;

use axum::{
	Router,
	extract::{Query, State},
	response::Redirect,
	routing::on,
};
use conduwuit_service::oauth::grant::{AuthorizationCodeQuery, Scope};
use ruma::OwnedUserId;
use url::Url;

use crate::{
	WebError,
	extract::{Expect, PostForm},
	pages::{
		GET_POST, Result,
		components::{Avatar, AvatarType},
	},
	response,
	session::{LoginQuery, LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/authorization_code", on(GET_POST, route_authorization_code))
}

template! {
	struct Grant use "grant.html.j2" {
		logout_query: String,
		user_id: OwnedUserId,
		user_avatar: Avatar,
		client_uri: Url,
		client_name: String,
		client_avatar: Avatar,
		policy_uri: Option<Url>,
		tos_uri: Option<Url>,
		scopes: BTreeSet<Scope>
	}
}

async fn route_authorization_code(
	State(services): State<crate::State>,
	user: User,
	Expect(Query(query)): Expect<Query<AuthorizationCodeQuery>>,
	PostForm(form): PostForm<()>,
) -> Result {
	let user_id = user.expect(LoginTarget::AuthorizationCode(query.clone()))?;

	if form.is_some() {
		let redirect_uri = services
			.oauth
			.request_authorization_code(user_id, query)
			.await
			.map_err(WebError::BadRequest)?;

		return response!(Redirect::to(&redirect_uri));
	}

	let Some(client) = services.oauth.get_client_metadata(&query.client_id).await else {
		return Err(WebError::BadRequest("Invalid client ID".to_owned()));
	};

	let scopes = query.scope.to_scopes().map_err(WebError::BadRequest)?;

	let client_name = if let Some(name) = &client.client_name {
		name
	} else {
		"Unknown application"
	}
	.to_owned();

	let client_avatar = {
		let avatar_type = if let Some(logo) = &client.logo_uri {
			AvatarType::Image(logo.to_string())
		} else if let Some(name) = &client.client_name
			&& let Some(char) = name.chars().next()
		{
			AvatarType::Initial(char)
		} else {
			AvatarType::Initial('?')
		};

		Avatar { avatar_type }
	};

	let user_avatar = Avatar::for_local_user(&services, &user_id).await;

	response!(Grant::new(
		&services,
		serde_urlencoded::to_string(LoginQuery {
			next: Some(LoginTarget::AuthorizationCode(query)),
			reauthenticate: false,
		})
		.unwrap(),
		user_id,
		user_avatar,
		client.client_uri.clone(),
		client_name,
		client_avatar,
		client.policy_uri.clone(),
		client.tos_uri.clone(),
		scopes,
	))
}
