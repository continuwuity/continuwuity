mod data;

use std::{sync::Arc, time::SystemTime};

use conduwuit::{Err, Result, utils};
use data::{Data, ResetTokenInfo};
use ruma::OwnedUserId;

use crate::{Dep, globals, users};

pub const PASSWORD_RESET_PATH: &str = "/_continuwuity/account/reset_password";
pub const RESET_TOKEN_QUERY_PARAM: &str = "token";
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
	pub async fn issue_token(&self, user_id: OwnedUserId) -> Result<ValidResetToken> {
		if !self.services.globals.user_is_local(&user_id) {
			return Err!("Cannot issue a password reset token for remote user {user_id}");
		}

		if user_id == self.services.globals.server_user {
			return Err!("Cannot issue a password reset token for the server user");
		}

		if self
			.services
			.users
			.origin(&user_id)
			.await
			.unwrap_or_else(|_| "password".to_owned())
			!= "password"
		{
			return Err!("Cannot issue a password reset token for non-internal user {user_id}");
		}

		if self.services.users.is_deactivated(&user_id).await? {
			return Err!("Cannot issue a password reset token for deactivated user {user_id}");
		}

		if let Some((existing_token, _)) = self.db.find_token_for_user(&user_id).await {
			self.db.remove_token(&existing_token);
		}

		let token = Self::generate_token_string();
		let info = ResetTokenInfo {
			user: user_id,
			issued_at: SystemTime::now(),
		};

		self.db.save_token(&token, &info);

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
