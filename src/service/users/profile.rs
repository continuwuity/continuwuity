use std::collections::BTreeMap;

use conduwuit::{
	Err, Result,
	pdu::PartialPdu,
	utils::{ReadyExt, stream::TryIgnore, to_canonical_object},
};
use database::{Deserialized, Ignore, Interfix, Json};
use futures::{Stream, StreamExt};
use ruma::{
	OwnedMxcUri, UserId,
	api::client::profile::PropagateTo,
	events::room::member::MembershipState,
	presence::PresenceState,
	profile::{ProfileFieldName, ProfileFieldValue},
};
use serde_json::{Value, to_value};

pub enum ProfileFieldChange {
	Set(ProfileFieldValue),
	Delete(ProfileFieldName),
}

impl ProfileFieldChange {
	fn field_name(&self) -> ProfileFieldName {
		match self {
			| &Self::Delete(ref name) => name.clone(),
			| &Self::Set(ref value) => value.field_name(),
		}
	}

	fn value(&self) -> Option<Value> {
		if let Self::Set(value) = self {
			Some(value.value().into_owned())
		} else {
			None
		}
	}
}

impl super::Service {
	pub async fn set_profile_field(
		&self,
		user_id: &UserId,
		change: ProfileFieldChange,
		propagate_to: PropagateTo,
	) -> Result<()> {
		const MAX_KEY_LENGTH_BYTES: usize = 255;
		const MAX_PROFILE_LENGTH_BYTES: usize = 65536;

		let field_name = change.field_name();

		// TODO: The spec mentions special error codes (M_PROFILE_TOO_LARGE,
		// M_KEY_TOO_LARGE) for profile field size limits, but they're not in its list
		// of error codes and Ruma doesn't have them. Should we return those, or is
		// M_TOO_LARGE okay?
		if field_name.as_str().len() > MAX_KEY_LENGTH_BYTES {
			return Err!(Request(TooLarge(
				"Individual profile keys must not exceed {MAX_KEY_LENGTH_BYTES} bytes in length."
			)));
		}

		// Serialize the entire profile as canonical JSON, including the new change,
		// to check if it exceeds 64 KiB
		{
			let mut full_profile = self.get_local_profile(user_id).await;

			match &change {
				| ProfileFieldChange::Set(value) => {
					full_profile.insert(
						value.field_name().as_str().to_owned(),
						value.value().clone().into_owned(),
					);
				},
				| ProfileFieldChange::Delete(key) => {
					full_profile.remove(key.as_str());
				},
			}

			if let Ok(canonical_profile) = to_canonical_object(full_profile) {
				if serde_json::to_string(&canonical_profile)
					.expect("should be able to serialize to string")
					.len() > MAX_PROFILE_LENGTH_BYTES
				{
					return Err!(
						"Profile data must not exceed {MAX_PROFILE_LENGTH_BYTES} bytes in \
						 length."
					);
				}
			} else {
				return Err!(Request(BadJson("Failed to canonicalize profile.")));
			}
		}

		// If the user is local and changed their displayname or avatar_url, update it
		// in all their joined rooms. This is done before updating their profile data
		// so we can check the old value of the field if `propagate_to` is `unchanged`.
		if matches!(field_name, ProfileFieldName::AvatarUrl | ProfileFieldName::DisplayName)
			&& matches!(propagate_to, PropagateTo::All | PropagateTo::Unchanged)
			&& self.services.globals.user_is_local(user_id)
		{
			let current_displayname = self.displayname(user_id).await.ok();
			let current_avatar_url = self.avatar_url(user_id).await.ok();

			let mut all_joined_rooms = self.services.state_cache.rooms_joined(user_id);

			while let Some(room_id) = all_joined_rooms.next().await {
				// TODO: this clobbers any custom fields on the event content
				let mut current_membership = self
					.services
					.state_accessor
					.get_member(&room_id, user_id)
					.await
					.expect("should be able to fetch membership event for joined room");

				assert_eq!(
					current_membership.membership,
					MembershipState::Join,
					"user should be joined"
				);

				// If `propagate_to` is `unchanged`, and the current value of the field we're
				// updating was changed from its global value in this room, skip it.
				if matches!(propagate_to, PropagateTo::Unchanged) {
					let field_changed_from_global = match field_name {
						| ProfileFieldName::AvatarUrl =>
							current_membership.avatar_url.as_ref() != current_avatar_url.as_ref(),
						| ProfileFieldName::DisplayName =>
							current_membership.displayname.as_ref()
								!= current_displayname.as_ref(),
						| _ => unreachable!(),
					};

					if field_changed_from_global {
						continue;
					}
				}

				let state_lock = self.services.state.mutex.lock(room_id.as_str()).await;

				// Preserve keys in accordance with the key copying rules
				current_membership.reason = None;
				current_membership.join_authorized_via_users_server = None;
				match &change {
					| ProfileFieldChange::Set(ProfileFieldValue::AvatarUrl(avatar_url)) => {
						current_membership.avatar_url = Some(avatar_url.clone());
					},
					| ProfileFieldChange::Set(ProfileFieldValue::DisplayName(displayname)) => {
						current_membership.displayname = Some(displayname.clone());
					},
					| ProfileFieldChange::Delete(ProfileFieldName::AvatarUrl) => {
						current_membership.avatar_url = None;
					},
					| ProfileFieldChange::Delete(ProfileFieldName::DisplayName) => {
						current_membership.displayname = None;
					},
					| _ => unreachable!(),
				}

				let _ = self
					.services
					.timeline
					.build_and_append_pdu(
						PartialPdu::state(user_id.to_string(), &current_membership),
						user_id,
						Some(&room_id),
						&state_lock,
					)
					.await;
			}

			if self.services.config.allow_local_presence {
				// Send a presence EDU to indicate the profile changed
				let _ = self
					.services
					.presence
					.ping_presence(user_id, &PresenceState::Online)
					.await;
			}
		}

		match change {
			| ProfileFieldChange::Set(ProfileFieldValue::DisplayName(displayname)) => {
				self.set_displayname(user_id, Some(displayname).filter(|dn| !dn.is_empty()));
			},
			| ProfileFieldChange::Set(ProfileFieldValue::AvatarUrl(avatar_url)) => {
				self.set_avatar_url(user_id, Some(avatar_url).filter(|av| av.is_valid()));
			},
			| ProfileFieldChange::Delete(ProfileFieldName::DisplayName) => {
				self.set_displayname(user_id, None);
			},
			| ProfileFieldChange::Delete(ProfileFieldName::AvatarUrl) => {
				self.set_avatar_url(user_id, None);
			},
			| other => self.set_profile_key(user_id, other.field_name().as_str(), other.value()),
		}

		Ok(())
	}

	pub async fn get_local_profile(&self, user_id: &UserId) -> BTreeMap<String, Value> {
		let mut profile = BTreeMap::new();

		// Get displayname and avatar_url independently because `all_profile_keys`
		// doesn't include them
		for field in [ProfileFieldName::AvatarUrl, ProfileFieldName::DisplayName] {
			let key = field.as_str().to_owned();

			if let Some(value) = self.get_local_profile_field(user_id, field).await {
				profile.insert(key, value.value().into_owned());
			}
		}

		// Insert all other profile fields
		let mut all_fields = self.all_profile_keys(user_id);

		while let Some((key, value)) = all_fields.next().await {
			profile.insert(key, value);
		}

		profile
	}

	pub async fn get_local_profile_field(
		&self,
		user_id: &UserId,
		field: ProfileFieldName,
	) -> Option<ProfileFieldValue> {
		let value = match field.clone() {
			| ProfileFieldName::AvatarUrl => self
				.avatar_url(user_id)
				.await
				.ok()
				.map(to_value)
				.transpose()
				.expect("converting avatar url to value should succeed"),
			| ProfileFieldName::DisplayName => self
				.displayname(user_id)
				.await
				.ok()
				.map(to_value)
				.transpose()
				.expect("converting displayname to value should succeed"),
			| other => self.profile_key(user_id, other.as_str()).await.ok(),
		}?;

		Some(
			ProfileFieldValue::new(field.as_str(), value)
				.expect("local profile field should be valid"),
		)
	}

	/// Returns the displayname of a user on this homeserver.
	pub async fn displayname(&self, user_id: &UserId) -> Result<String> {
		self.db.userid_displayname.get(user_id).await.deserialized()
	}

	/// Sets a new displayname or removes it if displayname is None. You still
	/// need to notify all rooms of this change.
	fn set_displayname(&self, user_id: &UserId, displayname: Option<String>) {
		if let Some(displayname) = displayname {
			self.db.userid_displayname.insert(user_id, displayname);
		} else {
			self.db.userid_displayname.remove(user_id);
		}
	}

	/// Get the `avatar_url` of a user.
	pub async fn avatar_url(&self, user_id: &UserId) -> Result<OwnedMxcUri> {
		self.db.userid_avatarurl.get(user_id).await.deserialized()
	}

	/// Sets a new avatar_url or removes it if avatar_url is None.
	fn set_avatar_url(&self, user_id: &UserId, avatar_url: Option<OwnedMxcUri>) {
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
	pub async fn profile_key(&self, user_id: &UserId, profile_key: &str) -> Result<Value> {
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
	) -> impl Stream<Item = (String, Value)> + 'a + Send {
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
	fn set_profile_key(
		&self,
		user_id: &UserId,
		profile_key: &str,
		profile_key_value: Option<Value>,
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
