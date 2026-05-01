use std::{collections::BTreeSet, time::SystemTime};

use askama::{Template, filters::HtmlSafe};
use base64::Engine;
use conduwuit_core::{result::FlatOk, utils};
use conduwuit_service::{
	Services,
	media::mxc::Mxc,
	oauth::{client_metadata::ClientMetadata, grant::Scope},
};
use ruma::{OwnedDeviceId, OwnedUserId, UserId, api::client::device::Device};

pub(super) mod form;

#[derive(Debug)]
pub(super) enum AvatarType {
	Initial(char),
	Image(String),
}

#[derive(Debug, Template)]
#[template(path = "_components/avatar.html.j2")]
pub(super) struct Avatar {
	pub(super) avatar_type: AvatarType,
}

impl HtmlSafe for Avatar {}

impl Avatar {
	pub(super) async fn for_local_user(services: &Services, user_id: &UserId) -> Self {
		let display_name = services.users.displayname(user_id).await.ok();

		let avatar_src = async {
			let avatar_url = services.users.avatar_url(user_id).await.ok()?;
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

		let avatar_type = if let Some(avatar_src) = avatar_src {
			AvatarType::Image(avatar_src)
		} else if let Some(initial) = display_name
			.as_ref()
			.and_then(|display_name| display_name.chars().next())
		{
			AvatarType::Initial(initial)
		} else {
			AvatarType::Initial(user_id.localpart().chars().next().unwrap())
		};

		Self { avatar_type }
	}

	pub(super) fn for_device(
		oauth_metadata: Option<&ClientMetadata>,
		display_name: Option<&str>,
	) -> Self {
		let avatar_src = oauth_metadata
			.and_then(|metadata| metadata.logo_uri.as_ref())
			.map(|uri| uri.as_str().to_owned());

		let avatar_type = if let Some(avatar_src) = avatar_src {
			AvatarType::Image(avatar_src)
		} else if let Some(initial) = display_name.and_then(|name| name.chars().next()) {
			if oauth_metadata.is_some() {
				AvatarType::Initial(initial)
			} else {
				AvatarType::Initial('❖')
			}
		} else {
			AvatarType::Initial('?')
		};

		Self { avatar_type }
	}
}

#[derive(Debug, Template)]
#[template(path = "_components/user_card.html.j2")]
pub(super) struct UserCard {
	pub user_id: OwnedUserId,
	pub display_name: Option<String>,
	pub avatar: Avatar,
}

impl HtmlSafe for UserCard {}

impl UserCard {
	pub(super) async fn for_local_user(services: &Services, user_id: OwnedUserId) -> Self {
		let display_name = services.users.displayname(&user_id).await.ok();
		let avatar = Avatar::for_local_user(services, &user_id).await;

		Self { user_id, display_name, avatar }
	}
}

#[derive(Debug, Template)]
#[template(path = "_components/device_card.html.j2")]
pub(super) struct DeviceCard {
	pub device_id: OwnedDeviceId,
	pub display_name: Option<String>,
	pub avatar: Avatar,
	pub last_active: String,
	pub oauth_metadata: Option<ClientMetadata>,
	pub style: DeviceCardStyle,
}

impl HtmlSafe for DeviceCard {}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum DeviceCardStyle {
	Minimal,
	Detailed,
}

impl DeviceCard {
	pub(super) async fn for_device(
		services: &Services,
		user_id: &UserId,
		device: Device,
		style: DeviceCardStyle,
	) -> Self {
		let oauth_metadata = async {
			let session_info = services
				.oauth
				.get_session_info_for_device(user_id, &device.device_id)
				.await?;

			Some(
				services
					.oauth
					.get_client_metadata(&session_info.client_id)
					.await
					.expect("client should exist"),
			)
		}
		.await;

		let display_name = oauth_metadata
			.as_ref()
			.and_then(|metadata| metadata.client_name.clone())
			.or_else(|| device.display_name.clone());

		let avatar = Avatar::for_device(oauth_metadata.as_ref(), display_name.as_deref());

		let last_active = device.last_seen_ts.map_or_else(
			|| "unknown".to_owned(),
			|last_seen_ts| {
				last_seen_ts
					.to_system_time()
					.and_then(|t| SystemTime::now().duration_since(t).ok())
					.map_or_else(
						|| "now".to_owned(),
						|duration| format!("{} ago", utils::time::pretty(duration)),
					)
			},
		);

		Self {
			device_id: device.device_id,
			display_name,
			avatar,
			last_active,
			oauth_metadata,
			style,
		}
	}
}

#[derive(Debug, Template)]
#[template(path = "_components/client_scopes.html.j2")]
pub(super) struct ClientScopes {
	pub scopes: BTreeSet<Scope>,
}

impl HtmlSafe for ClientScopes {}
