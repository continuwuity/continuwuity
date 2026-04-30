use axum::{
	Router,
	extract::{Path, State},
	routing::on,
};
use futures::StreamExt;
use ruma::OwnedDeviceId;
use serde::{Deserialize, Serialize};

use crate::{
	WebError,
	extract::{Expect, PostForm},
	pages::{GET_POST, Result, components::DeviceCard},
	response,
	session::{LoginTarget, User},
	template,
};

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/{device}/remove", on(GET_POST, route_remove_device))
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
pub(crate) struct RemoveDevicePath {
	pub device: OwnedDeviceId,
}

async fn route_remove_device(
	State(services): State<crate::State>,
	user: User,
	Expect(Path(query)): Expect<Path<RemoveDevicePath>>,
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
		let device_card = DeviceCard::for_device(&services, &user_id, device, false).await;
		let last_device = services.users.all_devices_metadata(&user_id).count().await <= 1;

		response!(RemoveDevice::new(&services, RemoveDeviceBody::Form {
			device_card: Box::new(device_card),
			last_device
		}))
	}
}
