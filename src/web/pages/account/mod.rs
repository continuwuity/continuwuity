use axum::{Router, extract::State, response::Response, routing::get};
use conduwuit_core::utils::{IterStream, stream::TryExpect};
use conduwuit_service::threepid::EmailRequirement;
use futures::StreamExt;
use ruma::{OwnedClientSecret, OwnedSessionId};
use serde::{Deserialize, Serialize};

use crate::{
	WebError,
	pages::components::{DeviceCard, UserCard},
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

async fn get_account(
	State(services): State<crate::State>,
	user: User,
) -> Result<Response, WebError> {
	let user_id = user.expect(LoginTarget::Account)?;

	let email_requirement = services.threepid.email_requirement();
	let email = services
		.threepid
		.get_email_for_localpart(user_id.localpart())
		.await
		.map(|address| address.to_string());

	let user_card = UserCard::for_local_user(&services, user_id.clone()).await;

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
		.collect()
		.await;

	devices.sort_unstable_by(|a, b| a.last_seen_ts.cmp(&b.last_seen_ts).reverse());

	let device_cards = devices
		.into_iter()
		.stream()
		.then(async |device| DeviceCard::for_device(&services, &user_id, device, true).await)
		.collect()
		.await;

	response!(Account::new(&services, user_card, email_requirement, email, device_cards))
}
