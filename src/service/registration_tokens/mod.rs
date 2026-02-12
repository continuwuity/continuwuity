mod data;

use std::{future::ready, pin::Pin, sync::Arc};

use conduwuit::{Err, Result, utils};
use data::Data;
pub use data::{DatabaseTokenInfo, TokenExpires};
use futures::{
	Stream, StreamExt,
	stream::{iter, once},
};
use ruma::OwnedUserId;

use crate::{Dep, config, firstrun};

const RANDOM_TOKEN_LENGTH: usize = 16;

pub struct Service {
	db: Data,
	services: Services,
}

struct Services {
	config: Dep<config::Service>,
	firstrun: Dep<firstrun::Service>,
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
	/// The single-use token which may be used to create the homeserver's first
	/// account.
	FirstAccount,
}

impl std::fmt::Display for ValidTokenSource {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			| Self::ConfigFile => write!(f, "Token defined in config."),
			| Self::Database(info) => info.fmt(f),
			| Self::FirstAccount => write!(f, "Initial setup token."),
		}
	}
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data::new(args.db),
			services: Services {
				config: args.depend::<config::Service>("config"),
				firstrun: args.depend::<firstrun::Service>("firstrun"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Generate a random string suitable to be used as a registration token.
	#[must_use]
	pub fn generate_token_string() -> String { utils::random_string(RANDOM_TOKEN_LENGTH) }

	/// Issue a new registration token and save it in the database.
	pub fn issue_token(
		&self,
		creator: OwnedUserId,
		expires: Option<TokenExpires>,
	) -> (String, DatabaseTokenInfo) {
		let token = Self::generate_token_string();
		let info = DatabaseTokenInfo::new(creator, expires);

		self.db.save_token(&token, &info);
		(token, info)
	}

	/// Get all the "special" registration tokens that aren't defined in the
	/// database.
	fn iterate_static_tokens(&self) -> impl Iterator<Item = ValidToken> {
		// This does not include the first-account token, because it's special:
		// no other registration tokens are valid when it is set.
		self.services.config.get_config_file_token().into_iter()
	}

	/// Validate a registration token.
	pub async fn validate_token(&self, token: String) -> Option<ValidToken> {
		// Check for the first-account token first
		if let Some(first_account_token) = self.services.firstrun.get_first_account_token() {
			if first_account_token == *token {
				return Some(first_account_token);
			}

			// If the first-account token is set, no other tokens are valid
			return None;
		}

		// Then static registration tokens
		for static_token in self.iterate_static_tokens() {
			if static_token == *token {
				return Some(static_token);
			}
		}

		// Then check the database
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
			| ValidTokenSource::Database(mut info) => {
				info.uses = info.uses.saturating_add(1);

				self.db.save_token(&token, &info);
			},
			| _ => {
				// Do nothing for other token sources.
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
				Err!(
					"The token set in the config file cannot be revoked. Edit the config file \
					 to change it."
				)
			},
			| ValidTokenSource::Database(_) => {
				self.db.revoke_token(&token);
				Ok(())
			},
			| ValidTokenSource::FirstAccount => {
				Err!("The initial setup token cannot be revoked.")
			},
		}
	}

	/// Iterate over all valid registration tokens.
	pub fn iterate_tokens(&self) -> Pin<Box<dyn Stream<Item = ValidToken> + Send + '_>> {
		// If the first-account token is set, no other tokens are valid
		if let Some(first_account_token) = self.services.firstrun.get_first_account_token() {
			return once(ready(first_account_token)).boxed();
		}

		let db_tokens = self
			.db
			.iterate_and_clean_tokens()
			.map(|(token, info)| ValidToken {
				token: token.to_owned(),
				source: ValidTokenSource::Database(info),
			});

		iter(self.iterate_static_tokens()).chain(db_tokens).boxed()
	}
}
