use axum::{
	Extension, Router,
	extract::{Query, State},
	response::Redirect,
	routing::on,
};
use conduwuit_service::oauth::{
	client_metadata::ClientMetadata,
	grant::{AuthorizationCodeQuery, DeviceCodeVerifyQuery, Prompt},
};
use ruma::OwnedUserId;
use serde::Deserialize;

use crate::{
	ROUTE_PREFIX, WebError,
	extract::{Expect, PostForm},
	pages::{
		GET_POST, Result, TemplateContext,
		account::register::{RegisterQuery, RequestedRegistrationFlow},
		components::{Avatar, ClientScopes, UserCard},
	},
	response,
	session::{LoginIntent, LoginQuery, LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/authorization_code", on(GET_POST, route_authorization_code))
		.route("/device_code", on(GET_POST, route_device_code))
}

template! {
	struct Grant use "grant.html.j2" {
		logout_query: String,
		user_id: OwnedUserId,
		user_avatar: Avatar,
		client_metadata: ClientMetadata,
		scopes: ClientScopes,
		device_code: Option<String>
	}
}

async fn route_authorization_code(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	user: User<true>,
	Expect(Query(query)): Expect<Query<AuthorizationCodeQuery>>,
	PostForm(form): PostForm<()>,
) -> Result {
	let user_id = if let Some(user) = user.into_session() {
		user.user_id
	} else {
		let is_first_run = services.firstrun.is_first_run();
		let next = LoginTarget::AuthorizationCode(query.clone());

		let uri = if query
			.prompt
			.is_some_and(|prompt| matches!(prompt, Prompt::Create))
			|| is_first_run
		{
			format!(
				"{}/account/register/?{}",
				ROUTE_PREFIX,
				serde_urlencoded::to_string(RegisterQuery {
					next: Some(next),
					flow: if is_first_run {
						Some(RequestedRegistrationFlow::Trusted)
					} else {
						None
					},
					..Default::default()
				})
				.unwrap()
			)
		} else {
			format!(
				"{}/account/login?{}",
				ROUTE_PREFIX,
				serde_urlencoded::to_string(LoginQuery {
					next: Some(next),
					..Default::default()
				})
				.unwrap()
			)
		};

		return response!(Redirect::to(&uri));
	};

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

	let user_avatar = Avatar::for_local_user(&services, &user_id).await;

	response!(Grant::new(
		context,
		serde_urlencoded::to_string(LoginQuery {
			next: Some(LoginTarget::AuthorizationCode(query)),
			intent: Some(LoginIntent::SwitchAccounts),
			..Default::default()
		})
		.unwrap(),
		user_id,
		user_avatar,
		client,
		ClientScopes { scopes },
		None,
	))
}

#[derive(Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
enum DeviceCodeForm {
	Lookup {
		user_code: String,
	},
	Confirm {
		device_code: String,
	},
}

template! {
	struct DeviceCodeGrant use "device_code_grant.html.j2" {
		user_card: UserCard,
		body: DeviceCodeGrantBody
	}
}

#[derive(Debug)]
enum DeviceCodeGrantBody {
	Lookup {
		user_code_error: bool,
	},
	Success,
}

async fn route_device_code(
	State(services): State<crate::State>,
	Extension(context): Extension<TemplateContext>,
	user: User<true>,
	Expect(Query(query)): Expect<Query<DeviceCodeVerifyQuery>>,
	PostForm(form): PostForm<DeviceCodeForm>,
) -> Result {
	let user_id = if let Some(user) = user.into_session() {
		user.user_id
	} else {
		let next = LoginTarget::DeviceCode(query.clone());

		let uri = format!(
			"{}/account/login?{}",
			ROUTE_PREFIX,
			serde_urlencoded::to_string(LoginQuery { next: Some(next), ..Default::default() })
				.unwrap()
		);

		return response!(Redirect::to(&uri));
	};

	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

	match (form, query.user_code.clone()) {
		| (None, Some(user_code)) | (Some(DeviceCodeForm::Lookup { user_code }), _) => {
			let Some(grant_info) = services.oauth.grant_info_for_user_code(&user_code).await
			else {
				return response!(DeviceCodeGrant::new(
					context,
					user_card,
					DeviceCodeGrantBody::Lookup { user_code_error: true }
				));
			};

			let user_avatar = Avatar::for_local_user(&services, &user_id).await;

			response!(Grant::new(
				context,
				serde_urlencoded::to_string(LoginQuery {
					next: Some(LoginTarget::DeviceCode(query)),
					intent: Some(LoginIntent::SwitchAccounts),
					..Default::default()
				})
				.unwrap(),
				user_id,
				user_avatar,
				grant_info.client_metadata,
				ClientScopes { scopes: grant_info.requested_scopes },
				Some(grant_info.device_code),
			))
		},
		| (Some(DeviceCodeForm::Confirm { device_code }), _) => {
			services
				.oauth
				.validate_device_code(user_id, &device_code)
				.await
				.map_err(WebError::BadRequest)?;

			response!(DeviceCodeGrant::new(context, user_card, DeviceCodeGrantBody::Success))
		},
		| (None, None) =>
			response!(DeviceCodeGrant::new(context, user_card, DeviceCodeGrantBody::Lookup {
				user_code_error: false
			})),
	}
}
