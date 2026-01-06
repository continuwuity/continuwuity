mod data;

use std::sync::Arc;

use conduwuit::{Err, Result, utils};
use data::Data;
pub use data::{DatabaseTokenInfo, TokenExpires};
use futures::{Stream, StreamExt, stream};
use ruma::OwnedUserId;

use crate::{Dep, config};

const RANDOM_TOKEN_LENGTH: usize = 16;

pub struct Service {
	db: Data,
	services: Services,
}

struct Services {
	config: Dep<config::Service>,
}

/// A validated registration token which may be used to create an account.
#[derive(Debug)]
pub struct ValidToken {
	pub token: String,
	pub source: ValidTokenSource,
}

impl std::fmt::Display for ValidToken {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "`{}` --- {}", self.token, &self.source)
	}
}

impl PartialEq<str> for ValidToken {
	fn eq(&self, other: &str) -> bool { self.token == other }
}

/// The source of a valid database token.
#[derive(Debug)]
pub enum ValidTokenSource {
	/// The static token set in the homeserver's config file, which is
	/// always valid.
	ConfigFile,
	/// A database token which has been checked to be valid.
	Database(DatabaseTokenInfo),
}

impl std::fmt::Display for ValidTokenSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			| Self::ConfigFile => write!(f, "Token defined in config."),
			| Self::Database(info) => info.fmt(f),
		}
	}
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data::new(args.db),
			services: Services {
				config: args.depend::<config::Service>("config"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Issue a new registration token and save it in the database.
	pub fn issue_token(
		&self,
		creator: OwnedUserId,
		expires: Option<TokenExpires>,
	) -> (String, DatabaseTokenInfo) {
		let token = utils::random_string(RANDOM_TOKEN_LENGTH);
		let info = DatabaseTokenInfo::new(creator, expires);

		self.db.save_token(&token, &info);
		(token, info)
	}

	/// Get the registration token set in the config file, if it exists.
	pub fn get_config_file_token(&self) -> Option<ValidToken> {
		self.services
			.config
			.registration_token
			.clone()
			.map(|token| ValidToken {
				token,
				source: ValidTokenSource::ConfigFile,
			})
	}

	/// Validate a registration token.
	pub async fn validate_token(&self, token: String) -> Option<ValidToken> {
		// Check the registration token in the config first
		if self
			.get_config_file_token()
			.is_some_and(|valid_token| valid_token == *token)
		{
			return Some(ValidToken {
				token,
				source: ValidTokenSource::ConfigFile,
			});
		}

		// Now check the database
		if let Some(token_info) = self.db.lookup_token_info(&token).await
			&& token_info.is_valid()
		{
			return Some(ValidToken {
				token,
				source: ValidTokenSource::Database(token_info),
			});
		}

		// Otherwise it's not valid
		None
	}

	/// Mark a valid token as having been used to create a new account.
	pub fn mark_token_as_used(&self, ValidToken { token, source }: ValidToken) {
		match source {
			| ValidTokenSource::ConfigFile => {
				// we don't track uses of the config file token, do nothing
			},
			| ValidTokenSource::Database(mut info) => {
				info.uses = info.uses.saturating_add(1);

				self.db.save_token(&token, &info);
			},
		}
	}

	/// Try to revoke a valid token.
	///
	/// Note that some tokens (like the one set in the config file) cannot be
	/// revoked.
	pub fn revoke_token(&self, ValidToken { token, source }: ValidToken) -> Result {
		match source {
			| ValidTokenSource::ConfigFile => {
				// the config file token cannot be revoked
				Err!(
					"The token set in the config file cannot be revoked. Edit the config file \
					 to change it."
				)
			},
			| ValidTokenSource::Database(_) => {
				self.db.revoke_token(&token);
				Ok(())
			},
		}
	}

	/// Iterate over all valid registration tokens.
	pub fn iterate_tokens(&self) -> impl Stream<Item = ValidToken> + Send + '_ {
		let db_tokens = self
			.db
			.iterate_and_clean_tokens()
			.map(|(token, info)| ValidToken {
				token: token.to_owned(),
				source: ValidTokenSource::Database(info),
			});

		stream::iter(self.get_config_file_token()).chain(db_tokens)
	}
}
