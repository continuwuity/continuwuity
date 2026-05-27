use axum::extract::State;
use conduwuit::{Err, Result};
use futures::join;
use ruma::api::client::admin::{is_user_locked, lock_user};

use crate::Ruma;

/// # `GET /_matrix/client/v1/admin/lock/{userId}`
///
/// Check the account lock status of a target user
pub(crate) async fn get_lock_status(
	State(services): State<crate::State>,
	body: Ruma<is_user_locked::v1::Request>,
) -> Result<is_user_locked::v1::Response> {
	let status = services.users.status(&body.user_id).await;

	status.ensure_active()?;

	Ok(is_user_locked::v1::Response::new(
		services.users.is_locked(&body.user_id).await?,
	))
}

/// # `PUT /_matrix/client/v1/admin/lock/{userId}`
///
/// Set the account lock status of a target user
pub(crate) async fn put_lock_status(
	State(services): State<crate::State>,
	body: Ruma<lock_user::v1::Request>,
) -> Result<lock_user::v1::Response> {
	let sender_user = body.identity.sender_user();

	let (status, target_admin) = join!(
		services.users.status(&body.user_id),
		services.users.is_admin(&body.user_id),
	);

	status.ensure_active()?;

	if sender_user.is_some_and(|sender_user| body.user_id == sender_user) {
		return Err!(Request(Forbidden("You cannot lock yourself")));
	}

	if target_admin {
		return Err!(Request(Forbidden("You cannot lock another server administrator")));
	}

	if services.users.is_locked(&body.user_id).await? == body.locked {
		// No change
		return Ok(lock_user::v1::Response::new(body.locked));
	}

	let action = if body.locked {
		services
			.users
			.suspend_account(&body.user_id, sender_user)
			.await;
		"locked"
	} else {
		services.users.unsuspend_account(&body.user_id).await;
		"unlocked"
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
