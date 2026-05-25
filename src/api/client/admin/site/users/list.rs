use axum::extract::State;
use conduwuit::{Err, utils::stream::WidebandExt};
use futures::StreamExt;
use ruminuwuity::admin::continuwuity::users;
use tokio::join;

use crate::router::Ruma;

/// # `GET /_continuwuity/admin/v1/users`
///
/// Lists all users on this homeserver.
pub(crate) async fn list_users_route(
	State(services): State<crate::State>,
	body: Ruma<users::list::v1::Request>,
) -> conduwuit::Result<users::list::v1::Response> {
	let sender_user = body.sender_user();

	if !services.users.is_admin(sender_user).await {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}

	let users = services
		.users
		.list_local_users()
		.skip(body.offset.unwrap_or_default())
		.take(body.limit.unwrap_or(100).min(100))
		.wide_filter_map(|user_id| async move {
			let (deactivated, suspended, locked, admin, login_disabled) = join!(
				services.users.is_deactivated(&user_id),
				services.users.is_suspended(&user_id),
				services.users.is_locked(&user_id),
				services.users.is_admin(&user_id),
				services.users.is_login_disabled(&user_id),
			);
			Some(users::list::v1::User {
				user_id: user_id.clone(),
				deactivated: deactivated.unwrap_or_default(),
				suspended: suspended.unwrap_or_default(),
				locked: locked.unwrap_or_default(),
				admin,
				login_disabled,
			})
		})
		.collect()
		.await;

	Ok(users::list::v1::Response::new(users))
}
