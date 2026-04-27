use axum::extract::State;
use axum_client_ip::ClientIp;
use conduwuit::{Err, Result, debug, err, utils};
use futures::StreamExt;
use ruma::{
	MilliSecondsSinceUnixEpoch, OwnedDeviceId,
	api::client::device::{
		self, delete_device, delete_devices, get_device, get_devices, update_device,
	},
};
use service::uiaa::Identity;

use crate::{Ruma, client::DEVICE_ID_LENGTH};

/// # `GET /_matrix/client/r0/devices`
///
/// Get metadata on all devices of the sender user.
pub(crate) async fn get_devices_route(
	State(services): State<crate::State>,
	body: Ruma<get_devices::v3::Request>,
) -> Result<get_devices::v3::Response> {
	let devices: Vec<device::Device> = services
		.users
		.all_devices_metadata(body.sender_user())
		.collect()
		.await;

	Ok(get_devices::v3::Response::new(devices))
}

/// # `GET /_matrix/client/r0/devices/{deviceId}`
///
/// Get metadata on a single device of the sender user.
pub(crate) async fn get_device_route(
	State(services): State<crate::State>,
	body: Ruma<get_device::v3::Request>,
) -> Result<get_device::v3::Response> {
	let device = services
		.users
		.get_device_metadata(body.sender_user(), &body.body.device_id)
		.await
		.map_err(|_| err!(Request(NotFound("Device not found."))))?;

	Ok(get_device::v3::Response::new(device))
}

/// # `PUT /_matrix/client/r0/devices/{deviceId}`
///
/// Updates the metadata on a given device of the sender user.
#[tracing::instrument(skip_all, fields(%client), name = "update_device", level = "debug")]
pub(crate) async fn update_device_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<update_device::v3::Request>,
) -> Result<update_device::v3::Response> {
	let sender_user = body.sender_user();
	let appservice = body.appservice_info.as_ref();

	match services
		.users
		.get_device_metadata(sender_user, &body.device_id)
		.await
	{
		| Ok(mut device) => {
			device.display_name.clone_from(&body.display_name);
			device.last_seen_ip.clone_from(&Some(client.to_string()));
			device
				.last_seen_ts
				.clone_from(&Some(MilliSecondsSinceUnixEpoch::now()));

			services
				.users
				.update_device_metadata(sender_user, &body.device_id, &device)
				.await?;

			Ok(update_device::v3::Response::new())
		},
		| Err(_) => {
			let Some(appservice) = appservice else {
				return Err!(Request(NotFound("Device not found.")));
			};

			debug!(
				"Creating new device for {sender_user} from appservice {} as device ID does not \
				 exist",
				appservice.registration.id
			);

			let device_id = OwnedDeviceId::from(utils::random_string(DEVICE_ID_LENGTH));

			services
				.users
				.create_device(
					sender_user,
					&device_id,
					&appservice.registration.as_token,
					None,
					Some(client.to_string()),
				)
				.await?;

			return Ok(update_device::v3::Response::new());
		},
	}
}

/// # `DELETE /_matrix/client/r0/devices/{deviceId}`
///
/// Deletes the given device.
///
/// - Requires UIAA to verify user password
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
pub(crate) async fn delete_device_route(
	State(services): State<crate::State>,
	body: Ruma<delete_device::v3::Request>,
) -> Result<delete_device::v3::Response> {
	let sender_user = body.sender_user();
	let appservice = body.appservice_info.as_ref();

	// Appservices get to skip UIAA for this endpoint
	if appservice.is_none() {
		// Prompt the user to confirm with their password using UIAA
		let _ = services
			.uiaa
			.authenticate_password(&body.auth, sender_user, body.sender_device(), None)
			.await?;
	}

	services
		.users
		.remove_device(sender_user, &body.device_id)
		.await;

	Ok(delete_device::v3::Response::new())
}

/// # `POST /_matrix/client/v3/delete_devices`
///
/// Deletes the given list of devices.
///
/// - Requires UIAA to verify user password.
///
/// For each device:
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
pub(crate) async fn delete_devices_route(
	State(services): State<crate::State>,
	body: Ruma<delete_devices::v3::Request>,
) -> Result<delete_devices::v3::Response> {
	let sender_user = body.sender_user();
	let appservice = body.appservice_info.as_ref();

	// Appservices get to skip UIAA for this endpoint
	if appservice.is_none() {
		// Prompt the user to confirm with their password using UIAA
		let _ = services
			.uiaa
			.authenticate_password(&body.auth, sender_user, body.sender_device(), None)
			.await?;
	}

	for device_id in &body.devices {
		services.users.remove_device(sender_user, device_id).await;
	}

	Ok(delete_devices::v3::Response::new())
}
