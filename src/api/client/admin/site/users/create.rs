use axum::extract::State;
use conduwuit::{
	Err, err, error, info,
	utils::{IterStream, stream::BroadbandExt},
	warn,
};
use futures::{FutureExt, StreamExt};
use ruma::UserId;
use ruminuwuity::admin::continuwuity::users;
use service::users::HashedPassword;

use crate::router::Ruma;

/// # `POST /_continuwuity/admin/v1/users/create`
///
/// Creates a new user.
pub(crate) async fn create_user_route(
	State(services): State<crate::State>,
	body: Ruma<users::create::v1::Request>,
) -> conduwuit::Result<users::create::v1::Response> {
	let sender_user = body.sender_user();

	if !services.users.is_admin(sender_user).await {
		return Err!(Request(Forbidden("Only server administrators can use this endpoint")));
	}
	let user_id =
		&UserId::parse_with_server_name(&body.localpart, services.globals.server_name())?;
	if services.users.is_active_local(user_id).await {
		return Err!(Conflict("A user with this username already exists"));
	}

	services
		.users
		.create_local_account(
			user_id,
			HashedPassword::new(&body.password)?,
			body.email
				.clone()
				.map(lettre::Address::try_from)
				.transpose()
				.map_err(|e| err!(Request(BadJson("Invalid email address: {e}"))))?,
		)
		.await;
	if body.suspended {
		services.users.suspend_account(user_id, sender_user).await;
	}
	if body.locked {
		services.users.lock_account(user_id, sender_user).await;
	}
	if body.login_disabled {
		services.users.disable_login(user_id);
	}
	if let Some(ref value) = body.display_name {
		services.users.set_profile_key(
			user_id,
			"displayname",
			Some(serde_json::to_value(value)?),
		);
	}
	if let Some(ref value) = body.avatar_url {
		services
			.users
			.set_profile_key(user_id, "avatar_url", Some(serde_json::to_value(value)?));
	}
	if body.admin {
		services
			.admin
			.make_user_admin(user_id)
			.await
			.inspect_err(|e| error!("failed to make new user {user_id} an admin: {e}"))
			.ok();
	}
	if !body.skip_auto_join {
		services.users.join_auto_join_rooms(user_id).await;
	}

	body.auto_join_rooms
		.clone()
		.into_iter()
		.stream()
		.broad_filter_map(|room| async move {
			services
				.rooms
				.alias
				.resolve_with_servers(&room, None)
				.await
				.inspect_err(|e| {
					warn!(
						"Failed to resolve room alias to room ID when attempting to auto join \
						 {room}: {e}"
					);
				})
				.ok()
		})
		.for_each_concurrent(None, |(room_id, servers)| async move {
			match services
				.rooms
				.membership
				.join_room(
					user_id,
					&room_id,
					Some("Automatically joining this room upon registration".to_owned()),
					servers.as_ref(),
				)
				.boxed()
				.await
			{
				| Err(e) => {
					warn!("Failed to automatically join {user_id} to {room_id}: {e}");
				},
				| _ => {
					info!("Automatically joined room {user_id} to {room_id}");
				},
			}
		})
		.await;

	Ok(users::create::v1::Response::new(user_id.to_owned()))
}
