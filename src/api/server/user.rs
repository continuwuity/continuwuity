use std::time::Duration;

use axum::extract::State;
use conduwuit::{Error, Result};
use futures::{FutureExt, StreamExt, TryFutureExt};
use ruma::api::{
	client::error::ErrorKind,
	federation::{
		device::get_devices::{self, v1::UserDevice},
		keys::{claim_keys, get_keys},
	},
};

use crate::{
	Ruma,
	client::{claim_keys_helper, get_keys_helper},
};

/// # `GET /_matrix/federation/v1/user/devices/{userId}`
///
/// Gets information on all devices of the user.
pub(crate) async fn get_devices_route(
	State(services): State<crate::State>,
	body: Ruma<get_devices::v1::Request>,
) -> Result<get_devices::v1::Response> {
	if !services.globals.user_is_local(&body.user_id) {
		return Err(Error::BadRequest(
			ErrorKind::InvalidParam,
			"Tried to access user from other server.",
		));
	}

	let user_id = &body.user_id;
	Ok(get_devices::v1::Response {
		user_id: user_id.clone(),
		stream_id: services
			.users
			.get_devicelist_version(user_id)
			.await
			.unwrap_or(0)
			.try_into()?,
		devices: services
			.users
			.all_devices_metadata(user_id)
			.filter_map(|metadata| async move {
				let device_id = metadata.device_id.clone();
				let device_id_clone = device_id.clone();
				let device_id_string = device_id.as_str().to_owned();
				let device_display_name = if services.globals.allow_device_name_federation() {
					metadata.display_name.clone()
				} else {
					Some(device_id_string)
				};

				services
					.users
					.get_device_keys(user_id, &device_id_clone)
					.map_ok(|keys| UserDevice { device_id, keys, device_display_name })
					.map(Result::ok)
					.await
			})
			.collect()
			.await,
		master_key: services
			.users
			.get_master_key(None, &body.user_id, &|u| u.server_name() == body.origin())
			.await
			.ok(),
		self_signing_key: services
			.users
			.get_self_signing_key(None, &body.user_id, &|u| u.server_name() == body.origin())
			.await
			.ok(),
	})
}

/// # `POST /_matrix/federation/v1/user/keys/query`
///
/// Gets devices and identity keys for the given users.
pub(crate) async fn get_keys_route(
	State(services): State<crate::State>,
	body: Ruma<get_keys::v1::Request>,
) -> Result<get_keys::v1::Response> {
	if body
		.device_keys
		.iter()
		.any(|(u, _)| !services.globals.user_is_local(u))
	{
		return Err(Error::BadRequest(
			ErrorKind::InvalidParam,
			"User does not belong to this server.",
		));
	}

	let result = get_keys_helper(
		&services,
		None,
		&body.device_keys,
		|u| Some(u.server_name()) == body.origin.as_deref(),
		services.globals.allow_device_name_federation(),
		Duration::from_secs(0),
	)
	.await?;

	Ok(get_keys::v1::Response {
		device_keys: result.device_keys,
		master_keys: result.master_keys,
		self_signing_keys: result.self_signing_keys,
	})
}

/// # `POST /_matrix/federation/v1/user/keys/claim`
///
/// Claims one-time keys.
pub(crate) async fn claim_keys_route(
	State(services): State<crate::State>,
	body: Ruma<claim_keys::v1::Request>,
) -> Result<claim_keys::v1::Response> {
	if body
		.one_time_keys
		.iter()
		.any(|(u, _)| !services.globals.user_is_local(u))
	{
		return Err(Error::BadRequest(
			ErrorKind::InvalidParam,
			"Tried to access user from other server.",
		));
	}

	let result =
		claim_keys_helper(&services, &body.one_time_keys, Duration::from_secs(0)).await?;

	Ok(claim_keys::v1::Response { one_time_keys: result.one_time_keys })
}
