use conduwuit::utils::{ReadyExt, stream::TryIgnore};
use database::{Deserialized, Ignore, Interfix, Json};
use futures::{Stream, StreamExt};
use ruma::{OwnedMxcUri, UserId};

impl super::Service {
	/// Returns the displayname of a user on this homeserver.
	pub async fn displayname(&self, user_id: &UserId) -> conduwuit::Result<String> {
		self.db.userid_displayname.get(user_id).await.deserialized()
	}

	/// Sets a new displayname or removes it if displayname is None. You still
	/// need to notify all rooms of this change.
	pub fn set_displayname(&self, user_id: &UserId, displayname: Option<String>) {
		if let Some(displayname) = displayname {
			self.db.userid_displayname.insert(user_id, displayname);
		} else {
			self.db.userid_displayname.remove(user_id);
		}
	}

	/// Get the `avatar_url` of a user.
	pub async fn avatar_url(&self, user_id: &UserId) -> conduwuit::Result<OwnedMxcUri> {
		self.db.userid_avatarurl.get(user_id).await.deserialized()
	}

	/// Sets a new avatar_url or removes it if avatar_url is None.
	pub fn set_avatar_url(&self, user_id: &UserId, avatar_url: Option<OwnedMxcUri>) {
		match avatar_url {
			| Some(avatar_url) => {
				self.db.userid_avatarurl.insert(user_id, &avatar_url);
			},
			| _ => {
				self.db.userid_avatarurl.remove(user_id);
			},
		}
	}

	/// Gets a specific user profile key
	pub async fn profile_key(
		&self,
		user_id: &UserId,
		profile_key: &str,
	) -> conduwuit::Result<serde_json::Value> {
		let key = (user_id, profile_key);
		self.db
			.useridprofilekey_value
			.qry(&key)
			.await
			.and_then(|handle| serde_json::from_slice(&handle).map_err(Into::into))
	}

	/// Gets all the user's profile keys and values in an iterator
	pub fn all_profile_keys<'a>(
		&'a self,
		user_id: &'a UserId,
	) -> impl Stream<Item = (String, serde_json::Value)> + 'a + Send {
		type KeyVal<'a> = ((Ignore, String), &'a [u8]);

		let prefix = (user_id, Interfix);
		self.db
			.useridprofilekey_value
			.stream_prefix(&prefix)
			.ignore_err()
			.map(|((_, key), value): KeyVal<'_>| Ok((key, serde_json::from_slice(value)?)))
			.ignore_err()
	}

	/// Sets a new profile key value, removes the key if value is None
	pub fn set_profile_key(
		&self,
		user_id: &UserId,
		profile_key: &str,
		profile_key_value: Option<serde_json::Value>,
	) {
		let key = (user_id, profile_key);

		if let Some(value) = profile_key_value {
			self.db.useridprofilekey_value.put(key, Json(value));
		} else {
			self.db.useridprofilekey_value.del(key);
		}
	}

	/// Clears all profile data for a user, including display name and avatar
	/// url.
	pub async fn clear_profile(&self, user_id: &UserId) {
		self.set_displayname(user_id, None);
		self.set_avatar_url(user_id, None);
		self.all_profile_keys(user_id)
			.ready_for_each(|(key, _)| self.set_profile_key(user_id, &key, None))
			.await;
	}
}
