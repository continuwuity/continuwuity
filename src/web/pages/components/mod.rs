use std::time::SystemTime;

use askama::{Template, filters::HtmlSafe};
use base64::Engine;
use conduwuit_core::{result::FlatOk, utils};
use conduwuit_service::{Services, media::mxc::Mxc, oauth::client_metadata::ClientMetadata};
use ruma::{OwnedDeviceId, OwnedUserId, UserId};

pub(super) mod form;

#[derive(Debug)]
pub(super) enum AvatarType<'a> {
	Initial(char),
	Image(&'a str),
}

#[derive(Debug, Template)]
#[template(path = "_components/avatar.html.j2")]
pub(super) struct Avatar<'a> {
	pub(super) avatar_type: AvatarType<'a>,
}

impl HtmlSafe for Avatar<'_> {}

#[derive(Debug, Template)]
#[template(path = "_components/user_card.html.j2")]
pub(super) struct UserCard {
	pub user_id: OwnedUserId,
	pub display_name: Option<String>,
	pub avatar_src: Option<String>,
}

impl HtmlSafe for UserCard {}

impl UserCard {
	pub(super) async fn for_local_user(services: &Services, user_id: OwnedUserId) -> Self {
		let display_name = services.users.displayname(&user_id).await.ok();

		let avatar_src = async {
			let avatar_url = services.users.avatar_url(&user_id).await.ok()?;
			let (server_name, media_id) = avatar_url.parts().ok()?;
			let file = services
				.media
				.get(&Mxc { media_id, server_name })
				.await
				.flat_ok()?;

			Some(format!(
				"data:{};base64,{}",
				file.content_type
					.unwrap_or_else(|| "application/octet-stream".to_owned()),
				file.content
					.map(|content| base64::prelude::BASE64_STANDARD.encode(content))
					.unwrap_or_default(),
			))
		}
		.await;

		Self { user_id, display_name, avatar_src }
	}

	fn avatar(&self) -> Avatar<'_> {
		let avatar_type = if let Some(ref avatar_src) = self.avatar_src {
			AvatarType::Image(avatar_src)
		} else if let Some(initial) = self
			.display_name
			.as_ref()
			.and_then(|display_name| display_name.chars().next())
		{
			AvatarType::Initial(initial)
		} else {
			AvatarType::Initial(self.user_id.localpart().chars().next().unwrap())
		};

		Avatar { avatar_type }
	}
}

#[derive(Debug, Template)]
#[template(path = "_components/device_card.html.j2")]
pub(super) struct DeviceCard {
	pub device_id: OwnedDeviceId,
	pub display_name: Option<String>,
	pub avatar_src: Option<String>,
	pub last_active: String,
	pub oauth_metadata: Option<ClientMetadata>,
}

impl HtmlSafe for DeviceCard {}

impl DeviceCard {
	pub(super) async fn for_device(
		services: &Services,
		user_id: &UserId,
		device_id: OwnedDeviceId,
	) -> Self {
		let device = services
			.users
			.get_device_metadata(user_id, &device_id)
			.await
			.ok();

		let oauth_metadata = async {
			let client_id = services.oauth.get_client_id_for_device(&device_id).await?;

			Some(
				services
					.oauth
					.get_client_registration(&client_id)
					.await
					.expect("client should exist"),
			)
		}
		.await;

		let display_name = oauth_metadata
			.as_ref()
			.and_then(|metadata| metadata.client_name.clone())
			.or(device
				.as_ref()
				.and_then(|device| device.display_name.clone()));

		let avatar_src = oauth_metadata
			.as_ref()
			.and_then(|metadata| metadata.logo_uri.as_ref())
			.map(|uri| uri.as_str().to_owned());

		let last_active = device
			.as_ref()
			.and_then(|device| device.last_seen_ts)
			.map_or_else(
				|| "unknown".to_owned(),
				|active| {
					active
						.to_system_time()
						.and_then(|t| SystemTime::now().duration_since(t).ok())
						.map_or_else(
							|| "now".to_owned(),
							|duration| format!("{} ago", utils::time::pretty(duration)),
						)
				},
			);

		Self {
			device_id,
			display_name,
			avatar_src,
			last_active,
			oauth_metadata,
		}
	}

	fn avatar(&self) -> Avatar<'_> {
		let avatar_type = if let Some(avatar_src) = &self.avatar_src {
			AvatarType::Image(avatar_src.as_str())
		} else if let Some(initial) = self
			.display_name
			.as_ref()
			.and_then(|name| name.chars().next())
		{
			if self.oauth_metadata.is_some() {
				AvatarType::Initial(initial)
			} else {
				AvatarType::Initial('❖')
			}
		} else {
			AvatarType::Initial('?')
		};

		Avatar { avatar_type }
	}
}
