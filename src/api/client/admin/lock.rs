use axum::extract::State;
use conduwuit::{Err, Result};
use futures::future::{join, join3};
use ruma::api::client::admin::{is_user_locked, lock_user};

use crate::Ruma;

/// # `GET /_matrix/client/v1/admin/lock/{userId}`
///
/// Check the account lock status of a target user
pub(crate) async fn get_lock_status(
	State(services): State<crate::State>,
	body: Ruma<is_user_locked::v1::Request>,
) -> Result<is_user_locked::v1::Response> {
	let (admin, active) = join(
		services.users.is_admin(body.identity.expect_sender_user()?),
		services.users.is_active(&body.user_id),
	)
	.await;
	if !admin {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}
	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("Can only check the lock status of local users")));
	}
	if !active {
		return Err!(Request(NotFound("Unknown user")));
	}
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
	let sender_user = body.identity.expect_sender_user()?;

	let (sender_admin, active, target_admin) = join3(
		services.users.is_admin(sender_user),
		services.users.is_active(&body.user_id),
		services.users.is_admin(&body.user_id),
	)
	.await;

	if !sender_admin {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}
	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("Can only set the lock status of local users")));
	}
	if !active {
		return Err!(Request(NotFound("Unknown user")));
	}
	if body.user_id == *sender_user {
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
			.send_text(&format!("{} has been {} by {}.", body.user_id, action, sender_user))
			.await;
	}

	Ok(lock_user::v1::Response::new(body.locked))
}
