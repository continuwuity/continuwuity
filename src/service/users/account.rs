use conduwuit::{
	Err, debug_error, debug_warn, err, trace, utils,
	utils::{ReadyExt, stream::TryIgnore},
};
use database::{Deserialized, Json};
use futures::{Stream, StreamExt, TryFutureExt};
use ruma::{
	MilliSecondsSinceUnixEpoch, OwnedDeviceId, OwnedUserId, UserId,
	events::{GlobalAccountDataEventType, ignored_user_list::IgnoredUserListEvent},
};
use ruminuwuity::invite_permission_config::{FilterLevel, InvitePermissionConfigEvent};

use crate::users::{HashedPassword, UserSuspension};

impl super::Service {
	/// Returns true/false based on whether the recipient/receiving user has
	/// ignored the sender.
	pub async fn user_is_ignored(&self, sender_user: &UserId, recipient_user: &UserId) -> bool {
		self.services
			.account_data
			.get_global(recipient_user, GlobalAccountDataEventType::IgnoredUserList)
			.await
			.is_ok_and(|ignored: IgnoredUserListEvent| {
				ignored
					.content
					.ignored_users
					.keys()
					.any(|blocked_user| blocked_user == sender_user)
			})
	}

	/// Returns the recipient's filter level for an invite from the sender.
	///
	/// If the sender is ignored by the recipient, `Ignore` is returned.
	/// Otherwise, the resulting value depends on their invite blocking or
	/// invite filtering configuration.
	pub async fn invite_filter_level(
		&self,
		sender_user: &UserId,
		recipient_user: &UserId,
	) -> FilterLevel {
		if self.user_is_ignored(sender_user, recipient_user).await {
			FilterLevel::Ignore
		} else {
			let (stable, unstable) = tokio::join!(
				self.services
					.account_data
					.get_global::<InvitePermissionConfigEvent>(
						recipient_user,
						GlobalAccountDataEventType::InvitePermissionConfig
					),
				self.services
					.account_data
					.get_global::<InvitePermissionConfigEvent>(
						recipient_user,
						"org.matrix.msc4155.invite_permission_config".into()
					) // TODO: MSC4155 probably needs upstreaming to ruma at some point
			);
			if stable.is_err() && unstable.is_err() {
				return FilterLevel::Allow;
			}
			stable
				.unwrap_or_else(|_| unstable.unwrap())
				.content
				.user_filter_level(sender_user)
		}
	}

	/// Check if a user is an admin
	#[inline]
	pub async fn is_admin(&self, user_id: &UserId) -> bool {
		self.services.admin.user_is_admin(user_id).await
	}

	/// Create a new user account on this homeserver. Set the password to `None`
	/// to create a non-local user. Non-local users with a password will return
	/// an error.
	#[inline]
	pub fn create(
		&self,
		user_id: &UserId,
		password: Option<HashedPassword>,
	) -> conduwuit::Result<()> {
		if !self.services.globals.user_is_local(user_id) && password.is_some() {
			return Err!("Cannot create a nonlocal user with a set password");
		}

		self.set_password(user_id, password);

		Ok(())
	}

	/// Deactivates an account, removing all of their device IDs and unsetting
	/// their password.
	pub async fn deactivate_account(&self, user_id: &UserId) -> conduwuit::Result<()> {
		// Remove all associated devices
		self.all_device_ids(user_id)
			.for_each(async |device_id| self.remove_device(user_id, &device_id).await)
			.await;

		// Set the password to "" to indicate a deactivated account. Hashes will never
		// result in an empty string, so the user will not be able to log in again.
		// Systems like changing the password without logging in should check if the
		// account is deactivated.
		self.set_password(user_id, None);

		// TODO: Unhook 3PID
		Ok(())
	}

	/// Suspend account, placing it in a read-only state
	pub async fn suspend_account(&self, user_id: &UserId, suspending_user: &UserId) {
		self.db.userid_suspension.raw_put(
			user_id,
			Json(UserSuspension {
				suspended: true,
				suspended_at: MilliSecondsSinceUnixEpoch::now().get().into(),
				suspended_by: suspending_user.to_string(),
			}),
		);
	}

	/// Unsuspend account, placing it in a read-write state
	pub async fn unsuspend_account(&self, user_id: &UserId) {
		self.db.userid_suspension.remove(user_id);
	}

	/// Locks an account, preventing it being used until it is unlocked.
	pub async fn lock_account(&self, user_id: &UserId, locking_user: &UserId) {
		// NOTE: Locking is basically just suspension with a more severe effect,
		// so we'll just re-use the suspension data structure to store the lock state.
		let suspension = self
			.db
			.userid_lock
			.get(user_id)
			.await
			.deserialized::<UserSuspension>()
			.unwrap_or_else(|_| UserSuspension {
				suspended: true,
				suspended_at: MilliSecondsSinceUnixEpoch::now().get().into(),
				suspended_by: locking_user.to_string(),
			});

		self.db.userid_lock.raw_put(user_id, Json(suspension));
	}

	/// Unlocks an account, allowing the user to log in and use it again.
	pub async fn unlock_account(&self, user_id: &UserId) { self.db.userid_lock.remove(user_id); }

	/// Check if the provided user ID belongs to an existing (possibly
	/// deactivated) account on this homeserver.
	#[inline]
	pub async fn exists(&self, user_id: &UserId) -> bool {
		self.services.globals.user_is_local(user_id)
			&& self.db.userid_password.get(user_id).await.is_ok()
	}

	/// Check if account is deactivated (has an empty password). Returns a
	/// NotFound error if the user does not exist.
	pub async fn is_deactivated(&self, user_id: &UserId) -> conduwuit::Result<bool> {
		self.db
			.userid_password
			.get(user_id)
			.map_ok(|val| val.is_empty())
			.map_err(|_| err!(Request(NotFound("User does not exist."))))
			.await
	}

	/// Check if account is suspended. Returns false if the user does not exist.
	pub async fn is_suspended(&self, user_id: &UserId) -> conduwuit::Result<bool> {
		match self
			.db
			.userid_suspension
			.get(user_id)
			.await
			.deserialized::<UserSuspension>()
		{
			| Ok(s) => Ok(s.suspended),
			| Err(e) =>
				if e.is_not_found() {
					Ok(false)
				} else {
					Err(e)
				},
		}
	}

	/// Returns true if the user is locked. Returns false if the user does not
	/// exist or is not locked.
	pub async fn is_locked(&self, user_id: &UserId) -> conduwuit::Result<bool> {
		match self
			.db
			.userid_lock
			.get(user_id)
			.await
			.deserialized::<UserSuspension>()
		{
			| Ok(s) => Ok(s.suspended),
			| Err(e) =>
				if e.is_not_found() {
					Ok(false)
				} else {
					Err(e)
				},
		}
	}

	/// Disables login for a user, preventing them from creating new devices,
	/// but allows them to continue using their existing sessions unimpeded.
	pub fn disable_login(&self, user_id: &UserId) {
		self.db.userid_logindisabled.insert(user_id, "");
	}

	/// Re-enables login for a user, allowing them to create new devices again.
	pub fn enable_login(&self, user_id: &UserId) { self.db.userid_logindisabled.remove(user_id); }

	/// Returns true if the target user's login is disabled.
	pub async fn is_login_disabled(&self, user_id: &UserId) -> bool {
		self.db
			.userid_logindisabled
			.exists(user_id.as_str())
			.await
			.is_ok()
	}

	/// Check if account is active (not deactivated)
	pub async fn is_active(&self, user_id: &UserId) -> bool {
		!self.is_deactivated(user_id).await.unwrap_or(true)
	}

	/// Check if account is a local user, and is active (not deactivated)
	pub async fn is_active_local(&self, user_id: &UserId) -> bool {
		self.services.globals.user_is_local(user_id) && self.is_active(user_id).await
	}

	/// Returns the number of users registered on this server, including
	/// deactivated users.
	#[inline]
	pub async fn count(&self) -> usize { self.db.userid_password.count().await }

	/// Find out which user an access token belongs to. Will panic if the access
	/// token is empty.
	pub async fn find_from_token(
		&self,
		token: &str,
	) -> conduwuit::Result<(OwnedUserId, OwnedDeviceId)> {
		assert!(!token.is_empty(), "Empty access token");
		self.db.token_userdeviceid.get(token).await.deserialized()
	}

	/// Returns an iterator over all users on this homeserver.
	pub fn stream(&self) -> impl Stream<Item = OwnedUserId> + Send {
		self.db.userid_password.keys().ignore_err()
	}

	/// Returns a list of active local users.
	///
	/// A user account is considered `local` if the associated password is not
	/// empty.
	pub fn list_local_users(&self) -> impl Stream<Item = OwnedUserId> + Send + '_ {
		self.db
			.userid_password
			.stream()
			.ignore_err()
			.ready_filter_map(|(u, p): (OwnedUserId, &[u8])| (!p.is_empty()).then_some(u))
	}

	/// Set a user's password.
	pub fn set_password(&self, user_id: &UserId, password: Option<HashedPassword>) {
		if let Some(hash) = password {
			self.db.userid_password.insert(user_id, hash.0);
		} else {
			self.db.userid_password.insert(user_id, b"");
		}
	}

	/// Check a user's password.
	pub async fn check_password(
		&self,
		user_id: &UserId,
		password: &str,
	) -> conduwuit::Result<OwnedUserId> {
		let (hash, user_id): (String, OwnedUserId) =
			if let Ok(hash) = self.db.userid_password.get(user_id).await.deserialized() {
				(hash, user_id.to_owned())
			} else {
				// We also check the lowercased version of the user ID to handle legacy user IDs
				// better
				let lowercase_user_id = UserId::parse(user_id.as_str().to_lowercase()).unwrap();

				if let Ok(hash) = self
					.db
					.userid_password
					.get(lowercase_user_id.as_str())
					.await
					.deserialized()
				{
					(hash, lowercase_user_id)
				} else {
					return Err!(Request(Forbidden("This user cannot log in with a password.")));
				}
			};

		if hash.is_empty() {
			return Err!(Request(UserDeactivated("This user is deactivated")));
		}

		utils::hash::verify_password(password, &hash)
			.inspect_err(|e| debug_error!("{e}"))
			.map_err(|_| err!(Request(Forbidden("Invalid identifier or password."))))?;

		Ok(user_id)
	}

	/// Creates an OpenID token, which can be used to prove that a user has
	/// access to an account (primarily for integrations)
	pub fn create_openid_token(&self, user_id: &UserId, token: &str) -> conduwuit::Result<u64> {
		use std::num::Saturating as Sat;

		let expires_in = self.services.server.config.openid_token_ttl;
		let expires_at = Sat(utils::millis_since_unix_epoch()) + Sat(expires_in) * Sat(1000);

		let mut value = expires_at.0.to_be_bytes().to_vec();
		value.extend_from_slice(user_id.as_bytes());

		self.db
			.openidtoken_expiresatuserid
			.insert(token.as_bytes(), value.as_slice());

		Ok(expires_in)
	}

	/// Find out which user an OpenID access token belongs to.
	pub async fn find_from_openid_token(&self, token: &str) -> conduwuit::Result<OwnedUserId> {
		let Ok(value) = self.db.openidtoken_expiresatuserid.get(token).await else {
			return Err!(Request(Unauthorized("OpenID token is unrecognised")));
		};

		let (expires_at_bytes, user_bytes) = value.split_at(0_u64.to_be_bytes().len());
		let expires_at =
			u64::from_be_bytes(expires_at_bytes.try_into().map_err(|e| {
				err!(Database("expires_at in openid_userid is invalid u64. {e}"))
			})?);

		if expires_at < utils::millis_since_unix_epoch() {
			debug_warn!("OpenID token is expired, removing");
			self.db.openidtoken_expiresatuserid.remove(token.as_bytes());

			return Err!(Request(Unauthorized("OpenID token is expired")));
		}

		let user_string = utils::string_from_bytes(user_bytes)
			.map_err(|e| err!(Database("User ID in openid_userid is invalid unicode. {e}")))?;

		OwnedUserId::try_from(user_string)
			.map_err(|e| err!(Database("User ID in openid_userid is invalid. {e}")))
	}

	/// Creates a short-lived login token, which can be used to log in using the
	/// `m.login.token` mechanism.
	pub fn create_login_token(&self, user_id: &UserId, token: &str) -> u64 {
		use std::num::Saturating as Sat;

		let expires_in = self.services.server.config.login_token_ttl;
		let expires_at = Sat(utils::millis_since_unix_epoch()) + Sat(expires_in);

		let value = (expires_at.0, user_id);
		self.db.logintoken_expiresatuserid.raw_put(token, value);

		expires_in
	}

	/// Find out which user a login token belongs to.
	/// Removes the token to prevent double-use attacks.
	pub async fn find_from_login_token(&self, token: &str) -> conduwuit::Result<OwnedUserId> {
		let Ok(value) = self.db.logintoken_expiresatuserid.get(token).await else {
			return Err!(Request(Forbidden("Login token is unrecognised")));
		};
		let (expires_at, user_id): (u64, OwnedUserId) = value.deserialized()?;

		if expires_at < utils::millis_since_unix_epoch() {
			trace!(%user_id, ?token, "Removing expired login token");

			self.db.logintoken_expiresatuserid.remove(token);

			return Err!(Request(Forbidden("Login token is expired")));
		}

		self.db.logintoken_expiresatuserid.remove(token);

		Ok(user_id)
	}
}
