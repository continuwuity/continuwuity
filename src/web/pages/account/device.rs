use axum::{
	Router,
	extract::{Path, State},
	routing::{get, on},
};
use conduwuit_service::oauth::{SessionInfo, client_metadata::ClientMetadata};
use futures::StreamExt;
use ruma::OwnedDeviceId;
use serde::{Deserialize, Serialize};

use crate::{
	WebError,
	extract::{Expect, PostForm},
	pages::{
		GET_POST, Result,
		components::{ClientScopes, DeviceCard, DeviceCardStyle},
	},
	response,
	session::{LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new()
		.route("/{device}/", get(get_device_info))
		.route("/{device}/remove", on(GET_POST, route_remove_device))
}

template! {
	struct DeviceInfo use "device_info.html.j2" {
		device_card: DeviceCard,
		client_metadata: Option<(ClientMetadata, SessionInfo)>
	}
}

async fn get_device_info(
	State(services): State<crate::State>,
	user: User,
	Expect(Path(query)): Expect<Path<DevicePath>>,
) -> Result {
	let user_id = user.expect(LoginTarget::RemoveDevice(query.clone()))?;

	let Ok(device) = services
		.users
		.get_device_metadata(&user_id, &query.device)
		.await
	else {
		return response!(WebError::BadRequest("Unknown device".to_owned()));
	};

	let client_metadata = async {
		let session_info = services
			.oauth
			.get_session_info_for_device(&user_id, &device.device_id)
			.await?;
		let client_metadata = services
			.oauth
			.get_client_metadata(&session_info.client_id)
			.await?;

		Some((client_metadata, session_info))
	}
	.await;

	let device_card =
		DeviceCard::for_device(&services, &user_id, device, DeviceCardStyle::Detailed).await;

	response!(DeviceInfo::new(&services, device_card, client_metadata))
}

template! {
	struct RemoveDevice use "remove_device.html.j2" {
		body: RemoveDeviceBody
	}
}

#[derive(Debug)]
enum RemoveDeviceBody {
	Form {
		device_card: Box<DeviceCard>,
		last_device: bool,
	},
	Success,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct DevicePath {
	pub device: OwnedDeviceId,
}

async fn route_remove_device(
	State(services): State<crate::State>,
	user: User,
	Expect(Path(query)): Expect<Path<DevicePath>>,
	PostForm(form): PostForm<()>,
) -> Result {
	let user_id = user.expect(LoginTarget::RemoveDevice(query.clone()))?;

	let Ok(device) = services
		.users
		.get_device_metadata(&user_id, &query.device)
		.await
	else {
		return response!(WebError::BadRequest("Unknown device".to_owned()));
	};

	if form.is_some() {
		services
			.users
			.remove_device(&user_id, &device.device_id)
			.await;

		response!(RemoveDevice::new(&services, RemoveDeviceBody::Success))
	} else {
		let device_card =
			DeviceCard::for_device(&services, &user_id, device, DeviceCardStyle::Minimal).await;
		let last_device = services.users.all_devices_metadata(&user_id).count().await <= 1;

		response!(RemoveDevice::new(&services, RemoveDeviceBody::Form {
			device_card: Box::new(device_card),
			last_device
		}))
	}
}
