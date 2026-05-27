use axum::extract::State;
use conduwuit::Err;
use ruma::api::client::admin::{is_user_locked, lock_user};

use crate::router::Ruma;

/// # `GET /_matrix/client/v1/admin/lock/{userId}`
///
/// Check the account lock status of a target user
pub(crate) async fn get_locked_status(
	State(services): State<crate::State>,
	body: Ruma<is_user_locked::v1::Request>,
) -> conduwuit::Result<is_user_locked::v1::Response> {
	if !services.users.is_active_local(&body.user_id).await {
		return Err!(Request(InvalidParam(
			"Can only check the lock status of active local users"
		)));
	}

	Ok(is_user_locked::v1::Response::new(
		services.users.is_locked(&body.user_id).await?,
	))
}

/// # `PUT /_matrix/client/v1/admin/lock/{userId}`
///
/// Set the account lock status of a target user
pub(crate) async fn put_locked_status(
	State(services): State<crate::State>,
	body: Ruma<lock_user::v1::Request>,
) -> conduwuit::Result<lock_user::v1::Response> {
	if !services.users.is_active_local(&body.user_id).await {
		return Err!(Request(InvalidParam(
			"Can only set the locked status of active local users"
		)));
	}

	if body.identity.sender_user() == Some(&body.user_id) {
		return Err!(Request(Forbidden("You cannot lock yourself")));
	}

	if services.users.is_admin(&body.user_id).await {
		return Err!(Request(Forbidden("You cannot lock another server administrator")));
	}

	if services.users.is_locked(&body.user_id).await? == body.locked {
		// No change
		return Ok(lock_user::v1::Response::new(body.locked));
	}

	let action = if body.locked {
		services
			.users
			.lock_account(&body.user_id, body.identity.sender_user())
			.await;
		"suspended"
	} else {
		services.users.unlock_account(&body.user_id).await;
		"unsuspended"
	};

	if services.config.admin_room_notices {
		// Notify the admin room that an account has been un/suspended
		services
			.admin
			.send_text(&format!("{} has been {} by {}.", body.user_id, action, body.identity))
			.await;
	}

	Ok(lock_user::v1::Response::new(body.locked))
}
