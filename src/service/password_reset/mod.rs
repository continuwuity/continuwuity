mod data;

use std::{sync::Arc, time::SystemTime};

use conduwuit::{Err, Result, info, utils};
use data::{Data, ResetTokenInfo};
use ruma::OwnedUserId;

use crate::{Dep, globals, users};

const RESET_TOKEN_LENGTH: usize = 32;

pub struct Service {
	db: Data,
	services: Services,
}

struct Services {
	users: Dep<users::Service>,
	globals: Dep<globals::Service>,
}

#[derive(Debug)]
pub struct ValidResetToken {
	pub token: String,
	pub info: ResetTokenInfo,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data::new(args.db),
			services: Services {
				users: args.depend::<users::Service>("users"),
				globals: args.depend::<globals::Service>("globals"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Generate a random string suitable to be used as a password reset token.
	#[must_use]
	pub fn generate_token_string() -> String { utils::random_string(RESET_TOKEN_LENGTH) }

	/// Issue a password reset token for `user`, who must be a local user with
	/// the `password` origin.
	pub async fn issue_token(&self, user: OwnedUserId) -> Result<ValidResetToken> {
		if !self.services.globals.user_is_local(&user) {
			return Err!("Cannot issue a password reset token for remote user {user}");
		}

		if self.services.users.origin(&user).await? != "password" {
			return Err!("Cannot issue a password reset token for non-internal user {user}");
		}

		if let Some((existing_token, _)) = self.db.find_token_for_user(&user).await {
			self.db.remove_token(&existing_token);
		}

		let token = Self::generate_token_string();
		let info = ResetTokenInfo { user, issued_at: SystemTime::now() };

		self.db.save_token(&token, &info);

		info!(?info.user, "Issued a password reset token");
		Ok(ValidResetToken { token, info })
	}

	/// Check if `token` represents a valid, non-expired password reset token.
	pub async fn check_token(&self, token: &str) -> Option<ValidResetToken> {
		self.db.lookup_token_info(token).await.and_then(|info| {
			if info.is_valid() {
				Some(ValidResetToken { token: token.to_owned(), info })
			} else {
				self.db.remove_token(token);
				None
			}
		})
	}

	/// Consume the supplied valid token, using it to change its user's password
	/// to `new_password`.
	pub async fn consume_token(
		&self,
		ValidResetToken { token, info }: ValidResetToken,
		new_password: &str,
	) -> Result<()> {
		if info.is_valid() {
			self.db.remove_token(&token);
			self.services
				.users
				.set_password(&info.user, Some(new_password))
				.await?;
		}

		Ok(())
	}
}
