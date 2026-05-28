use axum::extract::State;
use conduwuit::{
	err, error, info,
	utils::{IterStream, stream::BroadbandExt},
	warn,
};
use futures::{FutureExt, StreamExt};
use ruma::{api::client::profile::PropagateTo, profile::ProfileFieldValue};
use ruminuwuity::admin::continuwuity::users;
use service::users::{HashedPassword, ProfileFieldChange};

use crate::router::Ruma;

/// # `POST /_continuwuity/admin/v1/users/create`
///
/// Creates a new user.
pub(crate) async fn create_user(
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
		.create_shadow_account(user_id)
		.await?;

	services.users.convert_to_local_account(user_id, HashedPassword::new(&body.password)?).await?;

	if let Some(email) = &email {
		services.threepid.associate_localpart_email(user_id.localpart(), email).await?;
	}

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
		services.users.set_profile_field(
			user_id,
			ProfileFieldChange::Set(ProfileFieldValue::DisplayName(value.to_owned())),
			PropagateTo::None,
		);
	}
	if let Some(ref value) = body.avatar_url {
		services.users.set_profile_field(
			user_id,
			ProfileFieldChange::Set(ProfileFieldValue::AvatarUrl(value.to_owned())),
			PropagateTo::None,
		);
	}
	if body.admin {
		services
			.admin
			.make_user_admin(user_id)
			.await
			.inspect_err(|e| error!("failed to make new user {user_id} an admin: {e}"))
			.ok();
	}

	body.auto_join_rooms
		.clone()
		.into_iter()
		.stream()
		.chain(
			if body.skip_auto_join {
				vec![]
			} else {
				services.config.auto_join_rooms.clone()
			}.into_iter().stream()
		)
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
