use axum::extract::State;
use conduwuit::{Err, Result};
use ruma::api::client::admin::{get_suspended, set_suspended};

use crate::Ruma;

/// # `GET /_matrix/client/v1/admin/suspend/{userId}`
///
/// Check the suspension status of a target user
pub(crate) async fn get_suspended_status(
	State(services): State<crate::State>,
	body: Ruma<get_suspended::v1::Request>,
) -> Result<get_suspended::v1::Response> {
	let sender_user = body.sender_user();
	if !services.users.is_admin(sender_user).await {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}
	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("Can only check the suspended status of local users")));
	}
	if !services.users.is_active(&body.user_id).await {
		return Err!(Request(NotFound("Unknown user")));
	}
	Ok(get_suspended::v1::Response::new(
		services.users.is_suspended(&body.user_id).await?,
	))
}

/// # `PUT /_matrix/client/v1/admin/suspend/{userId}`
///
/// Set the suspension status of a target user
pub(crate) async fn put_suspended_status(
	State(services): State<crate::State>,
	body: Ruma<set_suspended::v1::Request>,
) -> Result<set_suspended::v1::Response> {
	let sender_user = body.sender_user();
	if !services.users.is_admin(sender_user).await {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}
	if !services.globals.user_is_local(&body.user_id) {
		return Err!(Request(InvalidParam("Can only set the suspended status of local users")));
	}
	if !services.users.is_active(&body.user_id).await {
		return Err!(Request(NotFound("Unknown user")));
	}
	if body.user_id == *sender_user {
		return Err!(Request(Forbidden("You cannot suspend yourself")));
	}
	if services.users.is_admin(&body.user_id).await {
		return Err!(Request(Forbidden("You cannot suspend another admin")));
	}
	if services.users.is_suspended(&body.user_id).await? == body.suspended {
		// No change
		return Ok(set_suspended::v1::Response::new(body.suspended));
	}

	let action = if body.suspended {
		services
			.users
			.suspend_account(&body.user_id, sender_user)
			.await;
		"suspended"
	} else {
		services.users.unsuspend_account(&body.user_id).await;
		"unsuspended"
	};

	if services.config.admin_room_notices {
		// Notify the admin room that an account has been un/suspended
		services
			.admin
			.send_text(&format!("{} has been {} by {}.", body.user_id, action, sender_user))
			.await;
	}

	Ok(set_suspended::v1::Response::new(body.suspended))
}
