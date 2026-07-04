use std::collections::BTreeMap;

use conduwuit::{
	Err, Error, Result, err,
	utils::{ReadyExt, stream::TryIgnore, string::Unquoted},
};
use database::{Deserialized, Ignore, Json};
use futures::{Stream, StreamExt, TryFutureExt};
use ruma::{
	DeviceId, OneTimeKeyAlgorithm, OneTimeKeyId, OneTimeKeyName, OwnedKeyId, OwnedOneTimeKeyId,
	OwnedUserId, RoomId, UInt, UserId,
	api::error::ErrorKind,
	encryption::{CrossSigningKey, DeviceKeys, OneTimeKey},
	serde::Raw,
};

use crate::users::{clean_signatures, parse_master_key, parse_user_signing_key};

impl super::Service {
	/// Adds a single one-time key to a device.
	pub async fn add_one_time_key(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		one_time_key_key: &OneTimeKeyId,
		one_time_key_value: &Raw<OneTimeKey>,
	) -> Result {
		// All devices have metadata
		// Only existing devices should be able to call this, but we shouldn't assert
		// either...
		let key = (user_id, device_id);
		if self.db.userdeviceid_metadata.qry(&key).await.is_err() {
			return Err!(Database(error!(
				%user_id,
				%device_id,
				"User does not exist or device has no metadata."
			)));
		}

		let mut key = user_id.as_bytes().to_vec();
		key.push(0xFF);
		key.extend_from_slice(device_id.as_bytes());
		key.push(0xFF);
		// TODO: Use DeviceKeyId::to_string when it's available (and update everything,
		// because there are no wrapping quotation marks anymore)
		key.extend_from_slice(
			serde_json::to_string(one_time_key_key)
				.expect("DeviceKeyId::to_string always works")
				.as_bytes(),
		);

		self.db
			.onetimekeyid_onetimekeys
			.raw_put(key, Json(one_time_key_value));

		let count = self.services.globals.next_count().unwrap();
		self.db.userid_lastonetimekeyupdate.raw_put(user_id, count);

		self.services.sync.wake(user_id).await;

		Ok(())
	}

	/// Save a fallback key for the given user, device, and algorithm
	/// This key will replace an existing fallback key.
	pub async fn add_fallback_key(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		fallback_key_id: &OneTimeKeyId,
		fallback_key: &Raw<OneTimeKey>,
		used: bool,
	) -> Result {
		// All devices have metadata
		// Only existing devices should be able to call this, but we shouldn't assert
		// either...
		let key = (user_id, device_id);
		if self.db.userdeviceid_metadata.qry(&key).await.is_err() {
			return Err!(Database(error!(
				%user_id,
				%device_id,
				"User does not exist or device has no metadata."
			)));
		}

		// There is one fallback key slot per user, per device, per algorithm
		// Therefore we use this as the DB key for this column
		let db_key = (user_id, device_id, fallback_key_id.algorithm());

		self.db
			.fallbackkeyid_fallbackkey
			.put(db_key, (used, fallback_key_id.as_str(), Json(fallback_key)));

		Ok(())
	}

	/// Returns the timestamp at when the last OTK update for the user was, or 0
	/// if the keys have never been updated.
	pub async fn last_one_time_keys_update(&self, user_id: &UserId) -> u64 {
		self.db
			.userid_lastonetimekeyupdate
			.get(user_id)
			.await
			.deserialized()
			.unwrap_or(0)
	}

	/// Consumes a one-time key belonging to the device of a given algorithm. If
	/// no one-time keys are available, a fallback key is returned instead (if
	/// available). If neither an OTK nor fallback key are available, NotFound
	/// is returned.
	pub async fn take_one_time_key(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		key_algorithm: &OneTimeKeyAlgorithm,
	) -> Result<(OwnedKeyId<OneTimeKeyAlgorithm, OneTimeKeyName>, Raw<OneTimeKey>)> {
		let count = self.services.globals.next_count()?.to_be_bytes();
		self.db.userid_lastonetimekeyupdate.insert(user_id, count);

		let mut prefix = user_id.as_bytes().to_vec();
		prefix.push(0xFF);
		prefix.extend_from_slice(device_id.as_bytes());
		prefix.push(0xFF);
		prefix.push(b'"'); // Annoying quotation mark
		prefix.extend_from_slice(key_algorithm.as_ref().as_bytes());
		prefix.push(b':');

		let one_time_key = self
			.db
			.onetimekeyid_onetimekeys
			.raw_stream_prefix(&prefix)
			.ignore_err()
			.next()
			.await
			.map(|(key, val)| {
				self.db.onetimekeyid_onetimekeys.remove(key);

				let key = key
					.rsplit(|&b| b == 0xFF)
					.next()
					.ok_or_else(|| err!(Database("OneTimeKeyId in db is invalid.")))
					.unwrap();

				let key = serde_json::from_slice(key)
					.map_err(|e| err!(Database("OneTimeKeyId in db is invalid. {e}")))
					.unwrap();

				let val = serde_json::from_slice(val)
					.map_err(|e| err!(Database("OneTimeKeys in db are invalid. {e}")))
					.unwrap();

				(key, val)
			});

		if let Some(result) = one_time_key {
			self.services.sync.wake(user_id).await;
			return Ok(result);
		}

		// No one-time key has been found. Look for a fallback key.

		let db_key = (user_id, device_id, key_algorithm);

		let fallback_key = self
			.db
			.fallbackkeyid_fallbackkey
			.qry(&db_key)
			.await
			.ok()
			.and_then(|handle| {
				handle
					.deserialized::<(bool, OwnedOneTimeKeyId, Raw<OneTimeKey>)>()
					.ok()
			});

		if let Some((used, fallback_key_id, fallback_key_value)) = fallback_key {
			if !used {
				// write the key to the database again to mark it as used
				self.add_fallback_key(
					user_id,
					device_id,
					&fallback_key_id,
					&fallback_key_value,
					true,
				)
				.await?;
			}

			self.services.sync.wake(user_id).await;
			return Ok((fallback_key_id, fallback_key_value));
		}

		Err(err!(Request(NotFound("No one-time key or fallback key found"))))
	}

	/// Returns the number of one-time keys the given device has. Does not count
	/// fallback keys.
	pub async fn count_one_time_keys(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
	) -> BTreeMap<OneTimeKeyAlgorithm, UInt> {
		type KeyVal<'a> = ((Ignore, Ignore, &'a Unquoted), Ignore);

		let mut algorithm_counts = BTreeMap::<OneTimeKeyAlgorithm, _>::new();
		let query = (user_id, device_id);
		self.db
			.onetimekeyid_onetimekeys
			.stream_prefix(&query)
			.ignore_err()
			.ready_for_each(|((_, _, device_key_id), _): KeyVal<'_>| {
				let one_time_key_id: &OneTimeKeyId = device_key_id
					.as_str()
					.try_into()
					.expect("Invalid DeviceKeyID in database");

				let count: &mut UInt = algorithm_counts
					.entry(one_time_key_id.algorithm())
					.or_default();

				*count = count.saturating_add(1_u32.into());
			})
			.await;

		algorithm_counts
	}

	/// Returns a list of *unused* fallback key types.
	pub async fn list_unused_fallback_key_types(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
	) -> Vec<OneTimeKeyAlgorithm> {
		type KeyVal = ((String, String, OneTimeKeyAlgorithm), (bool, String, Ignore));

		let mut query = user_id.as_bytes().to_vec();
		query.push(0xFF);
		query.extend_from_slice(device_id.as_bytes());
		query.push(0xFF);

		let mut unused_algorithms = Vec::new();

		self.db
			.fallbackkeyid_fallbackkey
			.stream_prefix(&query)
			.ignore_err()
			.ready_for_each(|((_, _, fallback_key_algorithm), (used, ..)): KeyVal| {
				if !used {
					unused_algorithms.push(fallback_key_algorithm);
				}
			})
			.await;

		unused_algorithms
	}

	/// Adds device identity keys to a device, overwriting existing ones if they
	/// exist. Dispatches a device key update.
	pub async fn add_device_keys(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		device_keys: &Raw<DeviceKeys>,
	) {
		let key = (user_id, device_id);

		self.db.keyid_key.put(key, Json(device_keys));
		self.mark_device_key_update(user_id).await;
	}

	/// Adds cross-signing keys for the given user. If notify is true, a device
	/// key update is dispatched.
	pub async fn add_cross_signing_keys(
		&self,
		user_id: &UserId,
		master_key: &Option<Raw<CrossSigningKey>>,
		self_signing_key: &Option<Raw<CrossSigningKey>>,
		user_signing_key: &Option<Raw<CrossSigningKey>>,
		notify: bool,
	) -> Result<()> {
		let mut prefix = user_id.as_bytes().to_vec();
		prefix.push(0xFF);

		if let Some(master_key) = master_key {
			let (master_key_key, _) = parse_master_key(user_id, master_key)?;

			self.db
				.keyid_key
				.insert(&master_key_key, master_key.json().get().as_bytes());

			self.db
				.userid_masterkeyid
				.insert(user_id.as_bytes(), &master_key_key);
		}

		// Self-signing key
		if let Some(self_signing_key) = self_signing_key {
			let mut self_signing_key_ids = self_signing_key
				.deserialize()
				.map_err(|e| err!(Request(InvalidParam("Invalid self signing key: {e:?}"))))?
				.keys
				.into_values();

			let self_signing_key_id = self_signing_key_ids.next().ok_or(Error::BadRequest(
				ErrorKind::InvalidParam,
				"Self signing key contained no key.",
			))?;

			if self_signing_key_ids.next().is_some() {
				return Err(Error::BadRequest(
					ErrorKind::InvalidParam,
					"Self signing key contained more than one key.",
				));
			}

			let mut self_signing_key_key = prefix.clone();
			self_signing_key_key.extend_from_slice(self_signing_key_id.as_bytes());

			self.db
				.keyid_key
				.insert(&self_signing_key_key, self_signing_key.json().get().as_bytes());

			self.db
				.userid_selfsigningkeyid
				.insert(user_id.as_bytes(), &self_signing_key_key);
		}

		// User-signing key
		if let Some(user_signing_key) = user_signing_key {
			let user_signing_key_id = parse_user_signing_key(user_signing_key)?;

			let user_signing_key_key = (user_id, &user_signing_key_id);
			self.db
				.keyid_key
				.put_raw(user_signing_key_key, user_signing_key.json().get().as_bytes());

			self.db
				.userid_usersigningkeyid
				.raw_put(user_id, user_signing_key_key);
		}

		if notify {
			self.mark_device_key_update(user_id).await;
		}

		Ok(())
	}

	/// Appends a new signature to the given key ID. Dispatches a new device key
	/// update.
	pub async fn sign_key(
		&self,
		target_id: &UserId,
		key_id: &str,
		signature: (String, String),
		sender_id: &UserId,
	) -> Result {
		let key = (target_id, key_id);

		let mut cross_signing_key: serde_json::Value = self
			.db
			.keyid_key
			.qry(&key)
			.await
			.map_err(|_| err!(Request(InvalidParam("Tried to sign nonexistent key"))))?
			.deserialized()
			.map_err(|e| err!(Database(debug_warn!("key in keyid_key is invalid: {e:?}"))))?;

		let signatures = cross_signing_key
			.get_mut("signatures")
			.ok_or_else(|| {
				err!(Database(debug_warn!("key in keyid_key has no signatures field")))
			})?
			.as_object_mut()
			.ok_or_else(|| {
				err!(Database(debug_warn!("key in keyid_key has invalid signatures field.")))
			})?
			.entry(sender_id.to_string())
			.or_insert_with(|| serde_json::Map::new().into());

		signatures
			.as_object_mut()
			.ok_or_else(|| {
				err!(Database(debug_warn!("signatures in keyid_key for a user is invalid.")))
			})?
			.insert(signature.0, signature.1.into());

		let key = (target_id, key_id);
		self.db.keyid_key.put(key, Json(cross_signing_key));

		self.mark_device_key_update(target_id).await;

		Ok(())
	}

	/// Returns a stream of changed keys between the two counts.
	#[inline]
	pub fn keys_changed<'a>(
		&'a self,
		user_id: &'a UserId,
		from: Option<u64>,
		to: Option<u64>,
	) -> impl Stream<Item = OwnedUserId> + Send + 'a {
		self.keys_changed_user_or_room(user_id.as_str(), from, to)
			.map(|(user_id, ..)| user_id)
	}

	/// Returns a stream of changed keys between the two counts in the given
	/// room specifically.
	#[inline]
	pub fn room_keys_changed<'a>(
		&'a self,
		room_id: &'a RoomId,
		from: Option<u64>,
		to: Option<u64>,
	) -> impl Stream<Item = (OwnedUserId, u64)> + Send + 'a {
		self.keys_changed_user_or_room(room_id.as_str(), from, to)
	}

	fn keys_changed_user_or_room<'a>(
		&'a self,
		user_or_room_id: &'a str,
		from: Option<u64>,
		to: Option<u64>,
	) -> impl Stream<Item = (OwnedUserId, u64)> + Send + 'a {
		type KeyVal<'a> = ((&'a str, u64), OwnedUserId);

		let from = from.unwrap_or(0);
		let to = to.unwrap_or(u64::MAX);
		let start = (user_or_room_id, from.saturating_add(1));
		self.db
			.keychangeid_userid
			.stream_from(&start)
			.ignore_err()
			.ready_take_while(move |((prefix, count), _): &KeyVal<'_>| {
				*prefix == user_or_room_id && *count <= to
			})
			.map(|((_, count), user_id): KeyVal<'_>| (user_id, count))
	}

	/// Marks that a user's device keys have been updated, so that other users
	/// can be notified of the change.
	pub async fn mark_device_key_update(&self, user_id: &UserId) {
		let count = self.services.globals.next_count().unwrap();

		self.services
			.state_cache
			.rooms_joined(user_id)
			// Don't send key updates to unencrypted rooms
			.filter_map(async |room_id| {
				if self.services.state_accessor.is_encrypted_room(&room_id).await {
					Some(room_id)
				} else {
					None
				}
			})
			.for_each(async |room_id| {
				let key = (&room_id, count);
				self.db.keychangeid_userid.put_raw(key, user_id);

				self.services.sync.wake_all_joined(&room_id).await;
			})
			.await;

		let key = (user_id, count);
		self.db.keychangeid_userid.put_raw(key, user_id);
	}

	/// Returns the device identity keys for a given device.
	pub async fn get_device_keys<'a>(
		&'a self,
		user_id: &'a UserId,
		device_id: &DeviceId,
	) -> Result<Raw<DeviceKeys>> {
		let key_id = (user_id, device_id);
		self.db.keyid_key.qry(&key_id).await.deserialized()
	}

	/// Gets a specific cross-signing key.
	pub async fn get_key<F>(
		&self,
		key_id: &[u8],
		sender_user: Option<&UserId>,
		user_id: &UserId,
		allowed_signatures: &F,
	) -> Result<Raw<CrossSigningKey>>
	where
		F: Fn(&UserId) -> bool + Send + Sync,
	{
		let key: serde_json::Value = self.db.keyid_key.get(key_id).await.deserialized()?;

		let cleaned = clean_signatures(key, sender_user, user_id, allowed_signatures)?;
		let raw_value = serde_json::value::to_raw_value(&cleaned)?;
		Ok(Raw::from_json(raw_value))
	}

	/// Gets a user's master cross-signing key.
	pub async fn get_master_key<F>(
		&self,
		sender_user: Option<&UserId>,
		user_id: &UserId,
		allowed_signatures: &F,
	) -> Result<Raw<CrossSigningKey>>
	where
		F: Fn(&UserId) -> bool + Send + Sync,
	{
		let key_id = self.db.userid_masterkeyid.get(user_id).await?;

		self.get_key(&key_id, sender_user, user_id, allowed_signatures)
			.await
	}

	/// Gets a user's self-signing cross-signing key.
	pub async fn get_self_signing_key<F>(
		&self,
		sender_user: Option<&UserId>,
		user_id: &UserId,
		allowed_signatures: &F,
	) -> Result<Raw<CrossSigningKey>>
	where
		F: Fn(&UserId) -> bool + Send + Sync,
	{
		let key_id = self.db.userid_selfsigningkeyid.get(user_id).await?;

		self.get_key(&key_id, sender_user, user_id, allowed_signatures)
			.await
	}

	/// Gets a user's user-signing cross-signing key.
	pub async fn get_user_signing_key(&self, user_id: &UserId) -> Result<Raw<CrossSigningKey>> {
		self.db
			.userid_usersigningkeyid
			.get(user_id)
			.and_then(|key_id| self.db.keyid_key.get(&*key_id))
			.await
			.deserialized()
	}
}
