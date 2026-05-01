use axum::{
	Router,
	extract::{Query, State},
	response::Redirect,
	routing::get,
};
use conduwuit_core::utils::{IterStream, ReadyExt, stream::TryExpect};
use conduwuit_service::threepid::EmailRequirement;
use futures::StreamExt;
use ruma::{
	OwnedClientSecret, OwnedDeviceId, OwnedSessionId,
	api::client::discovery::get_authorization_server_metadata::v1::AccountManagementAction,
};
use serde::{Deserialize, Serialize};

use crate::{
	WebError,
	extract::Expect,
	pages::{
		Result,
		components::{DeviceCard, DeviceCardStyle, UserCard},
	},
	response,
	session::{LoginTarget, User},
	template,
};

pub(crate) mod cross_signing_reset;
pub(crate) mod deactivate;
pub(crate) mod device;
pub(crate) mod email;
pub(crate) mod login;
pub(crate) mod password;

pub(crate) fn build() -> Router<crate::State> {
	#[allow(clippy::wildcard_imports)]
	use self::*;

	Router::new()
		.route("/", get(get_account))
		.route("/deeplink", get(get_account_deeplink))
		.merge(login::build())
		.nest("/password/", password::build())
		.nest("/email/", email::build())
		.nest("/cross_signing_reset", cross_signing_reset::build())
		.nest("/deactivate", deactivate::build())
		.nest("/device/", device::build())
}

#[derive(Deserialize, Serialize)]
struct ThreepidQuery {
	client_secret: OwnedClientSecret,
	session_id: OwnedSessionId,
}

template! {
	struct Account use "account.html.j2" {
		user_card: UserCard,
		email_requirement: EmailRequirement,
		email: Option<String>,
		devices: Vec<DeviceCard>
	}
}

async fn get_account(State(services): State<crate::State>, user: User) -> Result {
	let user_id = user.expect(LoginTarget::Account)?;

	let email_requirement = services.threepid.email_requirement();
	let email = services
		.threepid
		.get_email_for_localpart(user_id.localpart())
		.await
		.map(|address| address.to_string());

	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

	let dehydrated_device_id = services.users.get_dehydrated_device_id(&user_id).await.ok();

	let mut devices: Vec<_> = services
		.users
		.all_device_ids(&user_id)
		.then(async |device_id| {
			services
				.users
				.get_device_metadata(&user_id, &device_id)
				.await
		})
		.expect_ok()
		.ready_filter(|device| {
			dehydrated_device_id
				.as_ref()
				.is_none_or(|id| device.device_id != *id)
		})
		.collect()
		.await;

	devices.sort_unstable_by(|a, b| a.last_seen_ts.cmp(&b.last_seen_ts).reverse());

	let device_cards = devices
		.into_iter()
		.stream()
		.then(async |device| {
			DeviceCard::for_device(&services, &user_id, device, DeviceCardStyle::Minimal).await
		})
		.collect()
		.await;

	response!(Account::new(&services, user_card, email_requirement, email, device_cards))
}

#[derive(Deserialize)]
struct AccountDeeplinkQuery {
	action: Option<AccountManagementAction>,
	device_id: Option<OwnedDeviceId>,
}

async fn get_account_deeplink(
	Expect(Query(query)): Expect<Query<AccountDeeplinkQuery>>,
) -> Result {
	let redirect_target = match query.action.unwrap_or(AccountManagementAction::Profile) {
		| AccountManagementAction::AccountDeactivate => "deactivate".to_owned(),
		| AccountManagementAction::CrossSigningReset => "cross_signing_reset".to_owned(),
		| AccountManagementAction::DeviceDelete => {
			let Some(device_id) = query.device_id else {
				return response!(WebError::BadRequest(
					"A device ID is required for this action".to_owned()
				));
			};

			format!("device/{device_id}/delete")
		},
		| AccountManagementAction::DeviceView => {
			let Some(device_id) = query.device_id else {
				return response!(WebError::BadRequest(
					"A device ID is required for this action".to_owned()
				));
			};

			format!("device/{device_id}/")
		},
		| AccountManagementAction::DevicesList => "#devices".to_owned(),
		| AccountManagementAction::Profile => String::new(),
		| _ => return response!(WebError::BadRequest("Unknown action".to_owned())),
	};

	response!(Redirect::to(&format!("{}/account/{}", crate::ROUTE_PREFIX, redirect_target)))
}
