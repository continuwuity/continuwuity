mod namespace_regex;
mod registration_info;

use std::{collections::BTreeMap, iter::IntoIterator, sync::Arc};

use async_trait::async_trait;
use conduwuit::{Err, Result, err, utils::stream::IterStream};
use database::Map;
use futures::{Future, FutureExt, Stream, TryStreamExt};
use ruma::{RoomAliasId, RoomId, UserId, api::appservice::Registration};
use tokio::sync::{RwLock, RwLockReadGuard};

pub use self::{namespace_regex::NamespaceRegex, registration_info::RegistrationInfo};
use crate::{Dep, globals, sending, users};

pub struct Service {
	registration_info: RwLock<Registrations>,
	services: Services,
	db: Data,
}

struct Services {
	globals: Dep<globals::Service>,
	sending: Dep<sending::Service>,
	users: Dep<users::Service>,
}

struct Data {
	id_appserviceregistrations: Arc<Map>,
}

type Registrations = BTreeMap<String, RegistrationInfo>;

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			registration_info: RwLock::new(BTreeMap::new()),
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				sending: args.depend::<sending::Service>("sending"),
				users: args.depend::<users::Service>("users"),
			},
			db: Data {
				id_appserviceregistrations: args.db["id_appserviceregistrations"].clone(),
			},
		}))
	}

	async fn worker(self: Arc<Self>) -> Result {
		// First, collect all appservices to check for token conflicts
		let appservices: Vec<(String, Registration)> = self.iter_db_ids().try_collect().await?;

		// Check for appservice-to-appservice token conflicts
		for i in 0..appservices.len() {
			for j in i.saturating_add(1)..appservices.len() {
				if appservices[i].1.as_token == appservices[j].1.as_token {
					return Err!(Database(error!(
						"Token collision detected: Appservices '{}' and '{}' have the same token",
						appservices[i].0, appservices[j].0
					)));
				}
			}
		}

		// Process each appservice
		for (id, registration) in appservices {
			// During startup, resolve any token collisions in favour of appservices
			// by logging out conflicting user devices
			if let Ok((user_id, device_id)) = self
				.services
				.users
				.find_from_token(&registration.as_token)
				.await
			{
				conduwuit::warn!(
					"Token collision detected during startup: Appservice '{}' token was also \
					 used by user '{}' device '{}'. Logging out the user device to resolve \
					 conflict.",
					id,
					user_id.localpart(),
					device_id
				);

				self.services
					.users
					.remove_device(&user_id, &device_id)
					.await;
			}

			self.start_appservice(id, registration).await?;
		}

		Ok(())
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Starts an appservice, ensuring its sender_localpart user exists and is
	/// active. Creates the user if it doesn't exist, or reactivates it if it
	/// was deactivated. Then registers the appservice in memory for request
	/// handling.
	async fn start_appservice(&self, id: String, registration: Registration) -> Result {
		let appservice_user_id = UserId::parse_with_server_name(
			registration.sender_localpart.as_str(),
			self.services.globals.server_name(),
		)?;

		if !self.services.users.exists(&appservice_user_id).await {
			self.services
				.users
				.create(&appservice_user_id, None, None)
				.await?;
		} else if self
			.services
			.users
			.is_deactivated(&appservice_user_id)
			.await
			.unwrap_or(false)
		{
			// Reactivate the appservice user if it was accidentally deactivated
			self.services
				.users
				.set_password(&appservice_user_id, None)
				.await?;
		}

		self.registration_info
			.write()
			.await
			.insert(id, registration.try_into()?);

		Ok(())
	}

	/// Registers an appservice and returns the ID to the caller
	pub async fn register_appservice(
		&self,
		registration: &Registration,
		appservice_config_body: &str,
	) -> Result {
		//TODO: Check for collisions between exclusive appservice namespaces

		// Check for token collision with other appservices (allow re-registration of
		// same appservice)
		if let Ok(existing) = self.find_from_token(&registration.as_token).await {
			if existing.registration.id != registration.id {
				return Err(err!(Request(InvalidParam(
					"Cannot register appservice: Token is already used by appservice '{}'. \
					 Please generate a different token.",
					existing.registration.id
				))));
			}
		}

		// Prevent token collision with existing user tokens
		if self
			.services
			.users
			.find_from_token(&registration.as_token)
			.await
			.is_ok()
		{
			return Err(err!(Request(InvalidParam(
				"Cannot register appservice: The provided token is already in use by a user \
				 device. Please generate a different token for the appservice."
			))));
		}

		self.db
			.id_appserviceregistrations
			.insert(&registration.id, appservice_config_body);

		self.start_appservice(registration.id.clone(), registration.clone())
			.await?;

		Ok(())
	}

	/// Remove an appservice registration
	///
	/// # Arguments
	///
	/// * `service_name` - the registration ID of the appservice
	pub async fn unregister_appservice(&self, appservice_id: &str) -> Result {
		// removes the appservice registration info
		self.registration_info
			.write()
			.await
			.remove(appservice_id)
			.ok_or_else(|| err!("Appservice not found"))?;

		// remove the appservice from the database
		self.db.id_appserviceregistrations.del(appservice_id);

		// deletes all active requests for the appservice if there are any so we stop
		// sending to the URL
		self.services
			.sending
			.cleanup_events(Some(appservice_id), None, None)
			.await
	}

	pub async fn get_registration(&self, id: &str) -> Option<Registration> {
		self.registration_info
			.read()
			.await
			.get(id)
			.cloned()
			.map(|info| info.registration)
	}

	/// Returns Result to match users::find_from_token for select_ok usage
	pub async fn find_from_token(&self, token: &str) -> Result<RegistrationInfo> {
		self.read()
			.await
			.values()
			.find(|info| info.registration.as_token == token)
			.cloned()
			.ok_or_else(|| err!(Request(NotFound("Appservice token not found"))))
	}

	/// Checks if a given user id matches any exclusive appservice regex
	pub async fn is_exclusive_user_id(&self, user_id: &UserId) -> bool {
		self.read()
			.await
			.values()
			.any(|info| info.is_exclusive_user_match(user_id))
	}

	/// Checks if a given room alias matches any exclusive appservice regex
	pub async fn is_exclusive_alias(&self, alias: &RoomAliasId) -> bool {
		self.read()
			.await
			.values()
			.any(|info| info.aliases.is_exclusive_match(alias.as_str()))
	}

	/// Checks if a given room id matches any exclusive appservice regex
	///
	/// TODO: use this?
	#[allow(dead_code)]
	pub async fn is_exclusive_room_id(&self, room_id: &RoomId) -> bool {
		self.read()
			.await
			.values()
			.any(|info| info.rooms.is_exclusive_match(room_id.as_str()))
	}

	pub fn iter_ids(&self) -> impl Stream<Item = String> + Send {
		self.read()
			.map(|info| info.keys().cloned().collect::<Vec<_>>())
			.map(IntoIterator::into_iter)
			.map(IterStream::stream)
			.flatten_stream()
	}

	pub fn iter_db_ids(&self) -> impl Stream<Item = Result<(String, Registration)>> + Send {
		self.db
			.id_appserviceregistrations
			.keys()
			.and_then(move |id: &str| async move {
				Ok((id.to_owned(), self.get_db_registration(id).await?))
			})
	}

	pub async fn get_db_registration(&self, id: &str) -> Result<Registration> {
		self.db
			.id_appserviceregistrations
			.get(id)
			.await
			.and_then(|ref bytes| serde_yaml::from_slice(bytes).map_err(Into::into))
			.map_err(|e| err!(Database("Invalid appservice {id:?} registration: {e:?}")))
	}

	pub fn read(&self) -> impl Future<Output = RwLockReadGuard<'_, Registrations>> + Send {
		self.registration_info.read()
	}
}
