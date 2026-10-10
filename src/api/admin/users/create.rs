use axum::extract::State;
use conduwuit::{Err, error};
use ruma::{api::client::profile::PropagateTo, profile::ProfileFieldValue};
use ruminuwuity::admin::continuwuity::users::create::v1::{Request, Response};
use service::users::{HashedPassword, ProfileFieldChange};

use crate::router::Ruma;

/// # `POST /_continuwuity/admin/v1/users`
///
/// Creates a new user on the homeserver.
pub(crate) async fn create_user(
	State(services): State<crate::State>,
	body: Ruma<Request>,
) -> conduwuit::Result<Response> {
	let sender_user = body.identity.expect_sender_user()?;
	if !services.users.is_admin(sender_user).await {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}

	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("User ID does not belong to this server.")));
	}
	if services.users.status(&body.user_id).await.is_found() {
		return Err!(Request(UserInUse("An account with this user ID already exists.")));
	}

	services
		.users
		.create_local_account(
			&body.user_id,
			body.password
				.as_ref()
				.and_then(|raw| HashedPassword::new(raw).ok()),
			None,
			None,
			None,
		)
		.await?;
	if body.locked {
		services
			.users
			.lock_account(&body.user_id, sender_user)
			.await;
	}
	if body.suspended {
		services
			.users
			.suspend_account(&body.user_id, sender_user)
			.await;
	}
	for (key, value) in body.profile.clone().unwrap_or_default() {
		services
			.users
			.set_profile_field(
				&body.user_id,
				ProfileFieldChange::Set(ProfileFieldValue::new(&key, value)?),
				PropagateTo::All,
			)
			.await
			.inspect_err(|e| error!(error=?e, %key, "Failed to set profile field"))
			.ok();
	}
	if body.admin {
		services.admin.make_user_admin(&body.user_id).await?;
	}

	Ok(Response::default())
}
