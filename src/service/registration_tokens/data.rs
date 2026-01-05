use std::{sync::Arc, time::SystemTime};

use conduwuit::utils::stream::{ReadyExt, TryIgnore};
use database::{Database, Deserialized, Json, Map};
use futures::Stream;
use ruma::OwnedUserId;
use serde::{Deserialize, Serialize};

pub(super) struct Data {
	registrationtoken_info: Arc<Map>,
}

/// Metadata of a registration token.
#[derive(Debug, Serialize, Deserialize)]
pub struct DatabaseTokenInfo {
	/// The admin user who created this token.
	pub creator: OwnedUserId,
	/// The number of times this token has been used to create an account.
	pub uses: u64,
	/// When this token will expire, if it expires.
	pub expires: Option<TokenExpires>,
}

impl DatabaseTokenInfo {
	pub(super) fn new(creator: OwnedUserId, expires: Option<TokenExpires>) -> Self {
		Self { creator, uses: 0, expires }
	}

	/// Determine whether this token info represents a valid token, i.e. one
	/// that has not expired according to its [`Self::expires`] property. If
	/// [`Self::expires`] is [`None`], this function will always return `true`.
	#[must_use]
	pub fn is_valid(&self) -> bool {
		match self.expires {
			| Some(TokenExpires::AfterUses(max_uses)) => self.uses <= max_uses,
			| Some(TokenExpires::AfterTime(expiry_time)) => {
				let now = SystemTime::now();

				expiry_time >= now
			},
			| None => true,
		}
	}
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TokenExpires {
	AfterUses(u64),
	AfterTime(SystemTime),
}

impl Data {
	pub(super) fn new(db: &Arc<Database>) -> Self {
		Self {
			registrationtoken_info: db["registrationtoken_info"].clone(),
		}
	}

	/// Associate a registration token with its metadata in the database.
	pub(super) fn save_token(&self, token: &str, info: &DatabaseTokenInfo) {
		self.registrationtoken_info.put(token, Json(info));
	}

	/// Delete a registration token.
	pub(super) fn revoke_token(&self, token: &str) { self.registrationtoken_info.del(token); }

	/// Look up a registration token's metadata.
	pub(super) async fn lookup_token_info(&self, token: &str) -> Option<DatabaseTokenInfo> {
		self.registrationtoken_info
			.qry(token)
			.await
			.deserialized()
			.ok()
	}

	/// Iterate over all valid tokens and delete expired ones.
	pub(super) fn iterate_and_clean_tokens(
		&self,
	) -> impl Stream<Item = (&str, DatabaseTokenInfo)> + Send + '_ {
		self.registrationtoken_info
			.stream()
			.ignore_err()
			.ready_filter(|item: &(&str, DatabaseTokenInfo)| {
				if item.1.is_valid() {
					true
				} else {
					self.registrationtoken_info.del(item.0);
					false
				}
			})
	}
}
