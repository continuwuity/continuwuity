use std::time::{Duration, SystemTime};

use conduwuit::{
	Err, Result, debug_error, debug_warn, err, error, info, trace,
	utils::{self, ReadyExt, stream::TryIgnore},
	warn,
};
use database::{Deserialized, Json};
use futures::{FutureExt, Stream, StreamExt, TryFutureExt};
use lettre::Address;
use ruma::{
	MilliSecondsSinceUnixEpoch, OwnedDeviceId, OwnedUserId, UserId,
	events::{
		GlobalAccountDataEventType, ignored_user_list::IgnoredUserListEvent,
		push_rules::PushRulesEvent, room::message::RoomMessageEventContent,
	},
	push::Ruleset,
};
use ruminuwuity::invite_permission_config::{FilterLevel, InvitePermissionConfigEvent};

use crate::{
	appservice::RegistrationInfo,
	users::{HashedPassword, UserSuspension},
};

/// The status of an access token.
pub enum AccessTokenStatus {
	Valid,
	Expired,
}

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
	pub fn create(&self, user_id: &UserId, password: Option<HashedPassword>) -> Result<()> {
		if !self.services.globals.user_is_local(user_id) && password.is_some() {
			return Err!("Cannot create a nonlocal user with a set password");
		}

		self.set_password(user_id, password);

		Ok(())
	}

	/// Create a new account for a local human or bot user.
	pub async fn create_local_account(
		&self,
		user_id: &UserId,
		password: HashedPassword,
		email: Option<Address>,
	) {
		self.create(user_id, Some(password))
			.expect("should be able to save a new local user. what happened?");

		// Set an initial display name
		{
			let mut displayname = user_id.localpart().to_owned();

			let suffix = &self.services.config.new_user_displayname_suffix;
			if !suffix.is_empty() {
				displayname.push(' ');
				displayname.push_str(suffix);
			}

			self.set_displayname(user_id, Some(displayname));
		};

		// Set default push rules
		self.services
			.account_data
			.update(
				None,
				user_id,
				GlobalAccountDataEventType::PushRules.to_string().into(),
				&serde_json::to_value(PushRulesEvent::new(
					Ruleset::server_default(user_id).into(),
				))
				.expect("should be able to serialize push rules"),
			)
			.await
			.expect("should be able to update account data");

		// If the user registered with an email, associate it with their account.
		if let Some(email) = email {
			// This may fail if the email is already in use, but we should have already
			// checked that when we sent the validation email, so ignoring the error is
			// acceptable here in the rare case that an email is sniped by another user
			// between the validation email being sent and the account being created.
			let _ = self
				.services
				.threepid
				.associate_localpart_email(user_id.localpart(), &email)
				.await;
		}

		// Attempt to empower the first user and disable first-run mode.
		let was_first_user = self.services.firstrun.empower_first_user(user_id).await;

		// If the registering user was not the first and we're suspending users on
		// register, suspend them.
		if !was_first_user && self.services.config.suspend_on_register {
			// Note that we can still do auto joins for suspended users
			self.suspend_account(user_id, &self.services.globals.server_user)
				.await;

			// And send an @room notice to the admin room, to prompt admins to review the
			// new user and ideally unsuspend them if deemed appropriate.
			if self.services.config.admin_room_notices {
				self.services
					.admin
					.send_loud_message(RoomMessageEventContent::text_plain(format!(
						"User {user_id} has been suspended as they are not the first user on \
						 this server. Please review and unsuspend them if appropriate."
					)))
					.await
					.ok();
			}
		}

		// Autojoin the user to the configured autojoin rooms
		for room in &self.services.config.auto_join_rooms {
			let Ok(room_id) = self.services.alias.resolve(room).await else {
				error!(
					"Failed to resolve room alias to room ID when attempting to auto join \
					 {room}, skipping"
				);
				continue;
			};

			if !self
				.services
				.state_cache
				.server_in_room(self.services.globals.server_name(), &room_id)
				.await
			{
				warn!(
					"Skipping room {room} to automatically join as we have never joined before."
				);
				continue;
			}

			if let Some(room_server_name) = room.server_name() {
				match self
					.services
					.membership
					.join_room(
						user_id,
						&room_id,
						Some("Automatically joining this room upon registration".to_owned()),
						&[
							self.services.globals.server_name().to_owned(),
							room_server_name.to_owned(),
						],
					)
					.boxed()
					.await
				{
					| Err(e) => {
						// don't return this error so we don't fail registrations
						error!(
							"Failed to automatically join room {room} for user {user_id}: {e}"
						);
					},
					| _ => {
						info!("Automatically joined room {room} for user {user_id}");
					},
				}
			}
		}

		info!("Created new user account for {user_id}");
	}

	pub async fn determine_registration_user_id(
		&self,
		supplied_username: Option<String>,
		email: Option<&Address>,
		appservice_info: Option<&RegistrationInfo>,
	) -> Result<OwnedUserId> {
		const RANDOM_USER_ID_LENGTH: usize = 10;

		let emergency_mode_enabled = self.services.config.emergency_password.is_some();

		let supplied_username = supplied_username.or_else(|| {
			// If the user didn't supply a username but did supply an email, use
			// the email's user part to avoid falling back to a random username
			email.map(|address| address.user().to_owned())
		});

		if let Some(supplied_username) = supplied_username {
			// The user gets to pick their username. Do some validation to make sure it's
			// acceptable.

			// Don't allow registration with forbidden usernames.
			if self
				.services
				.globals
				.forbidden_usernames()
				.is_match(&supplied_username)
				&& !emergency_mode_enabled
			{
				return Err!(Request(Forbidden("Username is forbidden")));
			}

			// Create and validate the user ID
			let user_id = match UserId::parse_with_server_name(
				&supplied_username,
				self.services.globals.server_name(),
			) {
				| Ok(user_id) => {
					if let Err(e) = user_id.validate_strict() {
						// Unless we are in emergency mode, we should follow synapse's behaviour
						// on not allowing things like spaces and UTF-8 characters in
						// usernames
						if !emergency_mode_enabled {
							return Err!(Request(InvalidUsername(debug_warn!(
								"Username {supplied_username} contains disallowed characters or \
								 spaces: {e}"
							))));
						}
					}

					// Don't allow registration with user IDs that aren't local
					if !self.services.globals.user_is_local(&user_id) {
						return Err!(Request(InvalidUsername(
							"Username {supplied_username} is not local to this server"
						)));
					}

					user_id
				},
				| Err(e) => {
					return Err!(Request(InvalidUsername(debug_warn!(
						"Username {supplied_username} is not valid: {e}"
					))));
				},
			};

			if self.exists(&user_id).await {
				return Err!(Request(UserInUse("User ID is not available.")));
			}

			// Check that the user ID is/is not in an appservice's namespace
			if let Some(appservice_info) = appservice_info {
				if !appservice_info.is_user_match(&user_id) && !emergency_mode_enabled {
					return Err!(Request(Exclusive(
						"Username is not in this appservice's namespace."
					)));
				}
			} else if self
				.services
				.appservice
				.is_exclusive_user_id(&user_id)
				.await && !emergency_mode_enabled
			{
				return Err!(Request(Exclusive("Username is reserved by an appservice.")));
			}

			Ok(user_id)
		} else {
			// The user didn't specify a username. Generate a username for
			// them.

			loop {
				let user_id = UserId::parse_with_server_name(
					utils::random_string(RANDOM_USER_ID_LENGTH).to_lowercase(),
					self.services.globals.server_name(),
				)
				.unwrap();

				if !self.exists(&user_id).await {
					break Ok(user_id);
				}
			}
		}
	}

	/// Deactivates an account, removing all of their device IDs and unsetting
	/// their password.
	pub async fn deactivate_account(&self, user_id: &UserId) -> Result<()> {
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
	pub async fn is_deactivated(&self, user_id: &UserId) -> Result<bool> {
		self.db
			.userid_password
			.get(user_id)
			.map_ok(|val| val.is_empty())
			.map_err(|_| err!(Request(NotFound("User does not exist."))))
			.await
	}

	/// Check if account is suspended. Returns false if the user does not exist.
	pub async fn is_suspended(&self, user_id: &UserId) -> Result<bool> {
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
	pub async fn is_locked(&self, user_id: &UserId) -> Result<bool> {
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
	) -> Option<(OwnedUserId, OwnedDeviceId, AccessTokenStatus)> {
		assert!(!token.is_empty(), "Empty access token");

		let (user_id, device_id) = self
			.db
			.token_userdeviceid
			.get(token)
			.await
			.deserialized()
			.ok()?;

		// Check if the token has expired
		if let Some(expires) = self
			.db
			.userdeviceid_tokenexpires
			.qry(&(&user_id, &device_id))
			.await
			.deserialized::<u64>()
			.ok()
			.map(Duration::from_secs)
		{
			let expires_at = SystemTime::UNIX_EPOCH
				.checked_add(expires)
				.expect("expiry time should not overflow SystemTime");

			if SystemTime::now() > expires_at {
				return Some((user_id, device_id, AccessTokenStatus::Expired));
			}
		}

		Some((user_id, device_id, AccessTokenStatus::Valid))
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
	pub async fn check_password(&self, user_id: &UserId, password: &str) -> Result<OwnedUserId> {
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
	pub fn create_openid_token(&self, user_id: &UserId, token: &str) -> Result<u64> {
		use std::num::Saturating as Sat;

		let expires_in = self.services.config.openid_token_ttl;
		let expires_at = Sat(utils::millis_since_unix_epoch()) + Sat(expires_in) * Sat(1000);

		let mut value = expires_at.0.to_be_bytes().to_vec();
		value.extend_from_slice(user_id.as_bytes());

		self.db
			.openidtoken_expiresatuserid
			.insert(token.as_bytes(), value.as_slice());

		Ok(expires_in)
	}

	/// Find out which user an OpenID access token belongs to.
	pub async fn find_from_openid_token(&self, token: &str) -> Result<OwnedUserId> {
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

		let expires_in = self.services.config.login_token_ttl;
		let expires_at = Sat(utils::millis_since_unix_epoch()) + Sat(expires_in);

		let value = (expires_at.0, user_id);
		self.db.logintoken_expiresatuserid.raw_put(token, value);

		expires_in
	}

	/// Find out which user a login token belongs to.
	/// Removes the token to prevent double-use attacks.
	pub async fn find_from_login_token(&self, token: &str) -> Result<OwnedUserId> {
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
