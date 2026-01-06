use conduwuit::{Err, Result, utils};
use conduwuit_macros::admin_command;
use futures::StreamExt;
use service::registration_tokens::TokenExpires;

#[admin_command]
pub(super) async fn issue_token(&self, expires: super::TokenExpires) -> Result {
	let expires = {
		if expires.immortal {
			None
		} else if let Some(max_uses) = expires.max_uses {
			Some(TokenExpires::AfterUses(max_uses))
		} else if expires.once {
			Some(TokenExpires::AfterUses(1))
		} else if let Some(max_age) = expires
			.max_age
			.as_deref()
			.map(|max_age| utils::time::timepoint_from_now(utils::time::parse_duration(max_age)?))
			.transpose()?
		{
			Some(TokenExpires::AfterTime(max_age))
		} else {
			unreachable!();
		}
	};

	let (token, info) = self
		.services
		.registration_tokens
		.issue_token(self.sender_or_service_user().into(), expires);

	self.write_str(&format!(
		"New registration token issued: `{token}`. {}.",
		if let Some(expires) = info.expires {
			format!("{expires}")
		} else {
			"Never expires".to_owned()
		}
	))
	.await
}

#[admin_command]
pub(super) async fn revoke_token(&self, token: String) -> Result {
	let Some(token) = self
		.services
		.registration_tokens
		.validate_token(token)
		.await
	else {
		return Err!("This token does not exist or has already expired.");
	};

	self.services.registration_tokens.revoke_token(token)?;

	self.write_str("Token revoked successfully.").await
}

#[admin_command]
pub(super) async fn list_tokens(&self) -> Result {
	let tokens: Vec<_> = self
		.services
		.registration_tokens
		.iterate_tokens()
		.collect()
		.await;

	self.write_str(&format!("Found {} registration tokens:\n", tokens.len()))
		.await?;

	for token in tokens {
		self.write_str(&format!("- {token}\n")).await?;
	}

	Ok(())
}
