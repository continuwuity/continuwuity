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
	let email = body
		.email
		.clone()
		.map(lettre::Address::try_from)
		.transpose()
		.map_err(|e| err!(Request(BadJson("Invalid email address: {e}"))))?;

	let ref user_id = services
		.users
		.determine_registration_user_id(Some(body.localpart.clone()), email.as_ref(), None)
		.await?;

	services
		.users
		.create_local_account(user_id, HashedPassword::new(&body.password)?, email)
		.await;

	if body.suspended {
		services
			.users
			.suspend_account(&user_id, body.identity.sender_user())
			.await;
	}
	if body.locked {
		services
			.users
			.lock_account(user_id, body.identity.sender_user())
			.await;
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
