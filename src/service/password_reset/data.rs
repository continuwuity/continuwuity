use std::{
	sync::Arc,
	time::{Duration, SystemTime},
};

use conduwuit::utils::{ReadyExt, stream::TryExpect};
use database::{Database, Deserialized, Json, Map};
use ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};

pub(super) struct Data {
	passwordresettoken_info: Arc<Map>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResetTokenInfo {
	pub user: OwnedUserId,
	pub issued_at: SystemTime,
}

impl ResetTokenInfo {
	const MAX_TOKEN_AGE: Duration = Duration::from_secs(60 * 60);

	// one hour

	pub fn is_valid(&self) -> bool {
		let now = SystemTime::now();

		now.duration_since(self.issued_at)
			.is_ok_and(|duration| duration < Self::MAX_TOKEN_AGE)
	}
}

impl Data {
	pub(super) fn new(db: &Arc<Database>) -> Self {
		Self {
			passwordresettoken_info: db["passwordresettoken_info"].clone(),
		}
	}

	/// Associate a reset token with its info in the database.
	pub(super) fn save_token(&self, token: &str, info: &ResetTokenInfo) {
		self.passwordresettoken_info.raw_put(token, Json(info));
	}

	/// Lookup the info for a reset token.
	pub(super) async fn lookup_token_info(&self, token: &str) -> Option<ResetTokenInfo> {
		self.passwordresettoken_info
			.get(token)
			.await
			.deserialized()
			.ok()
	}

	/// Find a user's existing reset token, if any.
	pub(super) async fn find_token_for_user(
		&self,
		user: &UserId,
	) -> Option<(String, ResetTokenInfo)> {
		self.passwordresettoken_info
			.stream::<'_, String, ResetTokenInfo>()
			.expect_ok()
			.ready_find(|(_, info)| info.user == user)
			.await
	}

	/// Remove a reset token.
	pub(super) fn remove_token(&self, token: &str) { self.passwordresettoken_info.del(token); }
}
