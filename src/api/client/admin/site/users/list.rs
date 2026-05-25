use axum::extract::State;
use conduwuit::Err;
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

	let mut users = Vec::new();
	while let Some(user_id) = services.users.list_local_users().next().await {
		let (deactivated, suspended, locked, admin, login_disabled) = join!(
			services.users.is_deactivated(&user_id),
			services.users.is_suspended(&user_id),
			services.users.is_locked(&user_id),
			services.users.is_admin(&user_id),
			services.users.is_login_disabled(&user_id),
		);
		users.push(users::list::v1::User {
			user_id: user_id.clone(),
			deactivated: deactivated.unwrap_or_default(),
			suspended: suspended.unwrap_or_default(),
			locked: locked.unwrap_or_default(),
			admin,
			login_disabled,
		});
	}

	Ok(users::list::v1::Response::new(users))
}
