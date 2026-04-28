use askama::{Template, filters::HtmlSafe};
use base64::Engine;
use conduwuit_core::result::FlatOk;
use conduwuit_service::{Services, media::mxc::Mxc};
use ruma::UserId;

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
pub(super) struct UserCard<'a> {
	pub user_id: &'a UserId,
	pub display_name: Option<String>,
	pub avatar_src: Option<String>,
}

impl HtmlSafe for UserCard<'_> {}

impl<'a> UserCard<'a> {
	pub(super) async fn for_local_user(services: &Services, user_id: &'a UserId) -> Self {
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

		Self { user_id, display_name, avatar_src }
	}

	fn avatar(&'a self) -> Avatar<'a> {
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
