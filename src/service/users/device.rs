use std::{
	net::IpAddr,
	time::{Duration, SystemTime},
};

use conduwuit::{
	Err, Result,
	utils::{self, ReadyExt, stream::TryIgnore},
};
use database::{Deserialized, Ignore, Interfix, Json};
use futures::{Stream, StreamExt};
use ruma::{
	DeviceId, MilliSecondsSinceUnixEpoch, OwnedDeviceId, OwnedUserId, UserId,
	api::client::device::Device, assign, events::AnyToDeviceEvent, serde::Raw, uint,
};
use serde_json::json;

use crate::users::increment;

#[must_use]
#[derive(Clone)]
pub struct DeviceToken {
	token: String,
	max_age: Option<Duration>,
}

impl DeviceToken {
	const ACCESS_TOKEN_LENGTH: usize = 24;

	pub fn new(token: String) -> Self { Self { token, max_age: None } }

	pub fn new_random() -> Self { Self::new(utils::random_string(Self::ACCESS_TOKEN_LENGTH)) }

	pub fn with_max_age(self, max_age: Duration) -> Self {
		assign!(self, { max_age: Some(max_age) })
	}

	#[must_use]
	pub fn into_token(self) -> String { self.token }
}

impl super::Service {
	/// Adds a new device to a user.
	///
	/// If no `token` is provided, the device will not be able to be logged
	/// into.
	pub async fn create_device(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		token: Option<DeviceToken>,
		initial_device_display_name: Option<String>,
		client_ip: Option<String>,
	) -> Result<()> {
		self.status(user_id).await.ensure_active()?;

		let key = (user_id, device_id);
		let mut device = Device::new(device_id.into());
		device.display_name = initial_device_display_name;
		device.last_seen_ip = client_ip;
		device.last_seen_ts = Some(MilliSecondsSinceUnixEpoch::now());

		increment(&self.db.userid_devicelistversion, user_id.as_bytes());
		self.db.userdeviceid_metadata.put(key, Json(device));

		if let Some(token) = token {
			self.set_token(user_id, device_id, token).await?;
		}

		Ok(())
	}

	/// Removes a device from a user.
	pub async fn remove_device(&self, user_id: &UserId, device_id: &DeviceId) {
		// Remove dehydrated device if this is the dehydrated device
		let _ = self
			.remove_dehydrated_device(user_id, Some(device_id))
			.await;

		let userdeviceid = (user_id, device_id);

		// Remove tokens
		if let Ok(old_token) = self.db.userdeviceid_token.qry(&userdeviceid).await {
			self.db.userdeviceid_token.del(userdeviceid);
			self.db.token_userdeviceid.remove(&old_token);
			self.db.userdeviceid_tokenexpires.del(userdeviceid);
		}

		// Remove todevice events
		let prefix = (user_id, device_id, Interfix);
		self.db
			.todeviceid_events
			.keys_prefix_raw(&prefix)
			.ignore_err()
			.ready_for_each(|key| self.db.todeviceid_events.remove(key))
			.await;

		// TODO: Remove onetimekeys

		// Remove OAuth session information
		self.services.oauth.remove_session(user_id, device_id).await;

		increment(&self.db.userid_devicelistversion, user_id.as_bytes());

		self.db.userdeviceid_metadata.del(userdeviceid);
		self.mark_device_key_update(user_id).await;
	}

	/// Returns an iterator over all device ids of this user.
	pub fn all_device_ids<'a>(
		&'a self,
		user_id: &'a UserId,
	) -> impl Stream<Item = OwnedDeviceId> + Send + 'a {
		let prefix = (user_id, Interfix);
		self.db
			.userdeviceid_metadata
			.keys_prefix(&prefix)
			.ignore_err()
			.map(|(_, device_id): (Ignore, OwnedDeviceId)| device_id)
	}

	/// Gets the access token associated with a device.
	pub async fn get_token(&self, user_id: &UserId, device_id: &DeviceId) -> Result<String> {
		let key = (user_id, device_id);
		self.db.userdeviceid_token.qry(&key).await.deserialized()
	}

	/// Replaces the access token of one device.
	pub async fn set_token(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		DeviceToken { token, max_age }: DeviceToken,
	) -> Result<()> {
		let key = (user_id, device_id);
		if self.db.userdeviceid_metadata.qry(&key).await.is_err() {
			return Err!(Database(error!(
				%user_id,
				%device_id,
				"User does not exist or device has no metadata."
			)));
		}

		// Check for token collision with appservices
		if self
			.services
			.appservice
			.find_from_token(&token)
			.await
			.is_ok()
		{
			return Err!(Request(InvalidParam(
				"Token conflicts with an existing appservice token"
			)));
		}

		// Remove old token
		if let Ok(old_token) = self.db.userdeviceid_token.qry(&key).await {
			self.db.token_userdeviceid.remove(&old_token);
			self.db.userdeviceid_tokenexpires.remove(&old_token);
			// It will be removed from userdeviceid_token by the insert later
		}

		// Assign token to user device combination
		self.db.userdeviceid_token.put_raw(key, &token);
		self.db.token_userdeviceid.raw_put(&token, key);

		if let Some(max_age) = max_age {
			let expires = SystemTime::now()
				.duration_since(SystemTime::UNIX_EPOCH)
				.expect("system time should not be before the epoch")
				.saturating_add(max_age)
				.as_secs();

			self.db.userdeviceid_tokenexpires.put(key, expires);
		} else {
			self.db.userdeviceid_tokenexpires.del(key);
		}

		Ok(())
	}

	/// Pushes a new to-device event into a device's inbox.
	pub async fn add_to_device_event(
		&self,
		sender: &UserId,
		target_user_id: &UserId,
		target_device_id: &DeviceId,
		event_type: &str,
		content: serde_json::Value,
	) {
		let count = self.services.globals.next_count().unwrap();

		let key = (target_user_id, target_device_id, count);
		self.db.todeviceid_events.put(
			key,
			Json(json!({
				"type": event_type,
				"sender": sender,
				"content": content,
			})),
		);

		self.services.sync.wake(target_user_id).await;
	}

	/// Gets all to-device events between the two counts.
	pub fn get_to_device_events<'a>(
		&'a self,
		user_id: &'a UserId,
		device_id: &'a DeviceId,
		since: Option<u64>,
		to: Option<u64>,
	) -> impl Stream<Item = (u64, Raw<AnyToDeviceEvent>)> + Send + 'a {
		type Key = (OwnedUserId, OwnedDeviceId, u64);

		let from = (user_id, device_id, since.map_or(0, |since| since.saturating_add(1)));

		self.db
			.todeviceid_events
			.stream_from(&from)
			.ignore_err()
			.ready_take_while(move |((user_id_, device_id_, count), _): &(Key, _)| {
				user_id == *user_id_
					&& device_id == *device_id_
					&& to.is_none_or(|to| *count <= to)
			})
			.map(|((_, _, count), event)| (count, event))
	}

	/// Removes to-device events from the target device's inbox, until the given
	/// count.
	pub async fn remove_to_device_events<Until>(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		until: Until,
	) where
		Until: Into<Option<u64>> + Send,
	{
		type Key = (OwnedUserId, OwnedDeviceId, u64);

		let until = until.into().unwrap_or(u64::MAX);
		let from = (user_id, device_id, until);
		self.db
			.todeviceid_events
			.rev_keys_from(&from)
			.ignore_err()
			.ready_take_while(move |(user_id_, device_id_, _): &Key| {
				user_id == *user_id_ && device_id == *device_id_
			})
			.ready_for_each(|key: Key| {
				self.db.todeviceid_events.del(key);
			})
			.await;
	}

	/// Updates device metadata and increments the device list version.
	pub async fn update_device_metadata(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		device: &Device,
	) -> Result<()> {
		increment(&self.db.userid_devicelistversion, user_id.as_bytes());
		self.update_device_metadata_no_increment(user_id, device_id, device)
	}

	/// Updates device metadata without incrementing the device list version.
	/// This is namely used for updating the last_seen_ip and last_seen_ts
	/// values, as those do not need a device list version bump due to them not
	/// being relevant to other consumers.
	fn update_device_metadata_no_increment(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
		device: &Device,
	) -> Result<()> {
		let key = (user_id, device_id);
		self.db.userdeviceid_metadata.put(key, Json(device));

		Ok(())
	}

	/// Updates the last seen timestamp for a device. Silently does nothing if
	/// the last update was less than 10 seconds ago, or the device does not
	/// exist.
	pub async fn update_device_last_seen(
		&self,
		user_id: &UserId,
		device_id: Option<&DeviceId>,
		ip: IpAddr,
	) {
		let now = MilliSecondsSinceUnixEpoch::now();
		if let Some(device_id) = device_id {
			if let Ok(mut device) = self.get_device_metadata(user_id, device_id).await {
				device.last_seen_ip = Some(ip.to_string());
				// If the last update was less than 10 seconds ago, don't update the timestamp
				if let Some(prev) = device.last_seen_ts {
					if now.get().saturating_sub(prev.get()) < uint!(10_000) {
						return;
					}
				}
				device.last_seen_ts = Some(now);

				self.update_device_metadata_no_increment(user_id, device_id, &device)
					.ok();
			}
		}
	}

	/// Get device metadata.
	pub async fn get_device_metadata(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
	) -> Result<Device> {
		self.db
			.userdeviceid_metadata
			.qry(&(user_id, device_id))
			.await
			.deserialized()
	}

	/// Gets the most recent device list version for a user.
	pub async fn get_devicelist_version(&self, user_id: &UserId) -> Result<u64> {
		self.db
			.userid_devicelistversion
			.get(user_id)
			.await
			.deserialized()
	}

	/// Gets metadata for all devices belonging to the target user.
	pub fn all_devices_metadata<'a>(
		&'a self,
		user_id: &'a UserId,
	) -> impl Stream<Item = Device> + Send + 'a {
		let key = (user_id, Interfix);
		self.db
			.userdeviceid_metadata
			.stream_prefix(&key)
			.ignore_err()
			.map(|(_, val): (Ignore, Device)| val)
	}
}
