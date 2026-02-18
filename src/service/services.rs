use std::{any::Any, collections::BTreeMap, sync::Arc};

use conduwuit::{
	Result, Server, SyncRwLock, debug, debug_info, info, trace, utils::stream::IterStream,
};
use database::Database;
use futures::{Stream, StreamExt, TryStreamExt};
use tokio::sync::Mutex;

use crate::{
	account_data, admin, announcements, antispam, appservice, client, config, emergency,
	federation, firstrun, globals, key_backups,
	manager::Manager,
	media, moderation, presence, pusher, registration_tokens, resolver, rooms, sending,
	server_keys,
	service::{self, Args, Map, Service},
	sync, transaction_ids, uiaa, users,
};

pub struct Services {
	pub account_data: Arc<account_data::Service>,
	pub admin: Arc<admin::Service>,
	pub appservice: Arc<appservice::Service>,
	pub config: Arc<config::Service>,
	pub client: Arc<client::Service>,
	pub emergency: Arc<emergency::Service>,
	pub globals: Arc<globals::Service>,
	pub key_backups: Arc<key_backups::Service>,
	pub media: Arc<media::Service>,
	pub presence: Arc<presence::Service>,
	pub pusher: Arc<pusher::Service>,
	pub registration_tokens: Arc<registration_tokens::Service>,
	pub resolver: Arc<resolver::Service>,
	pub rooms: rooms::Service,
	pub federation: Arc<federation::Service>,
	pub firstrun: Arc<firstrun::Service>,
	pub sending: Arc<sending::Service>,
	pub server_keys: Arc<server_keys::Service>,
	pub sync: Arc<sync::Service>,
	pub transaction_ids: Arc<transaction_ids::Service>,
	pub uiaa: Arc<uiaa::Service>,
	pub users: Arc<users::Service>,
	pub moderation: Arc<moderation::Service>,
	pub announcements: Arc<announcements::Service>,
	pub antispam: Arc<antispam::Service>,

	manager: Mutex<Option<Arc<Manager>>>,
	pub(crate) service: Arc<Map>,
	pub server: Arc<Server>,
	pub db: Arc<Database>,
}

impl Services {
	#[allow(clippy::cognitive_complexity)]
	pub async fn build(server: Arc<Server>) -> Result<Arc<Self>> {
		let db = Database::open(&server).await?;
		let service: Arc<Map> = Arc::new(SyncRwLock::new(BTreeMap::new()));
		macro_rules! build {
			($tyname:ty) => {{
				let built = <$tyname>::build(Args {
					db: &db,
					server: &server,
					service: &service,
				})?;
				add_service(&service, built.clone(), built.clone());
				built
			}};
		}

		Ok(Arc::new(Self {
			// firstrun service should be built first so other services
			// can check first-run state
			firstrun: build!(firstrun::Service),
			account_data: build!(account_data::Service),
			admin: build!(admin::Service),
			appservice: build!(appservice::Service),
			resolver: build!(resolver::Service),
			client: build!(client::Service),
			config: build!(config::Service),
			emergency: build!(emergency::Service),
			globals: build!(globals::Service),
			key_backups: build!(key_backups::Service),
			media: build!(media::Service),
			presence: build!(presence::Service),
			pusher: build!(pusher::Service),
			registration_tokens: build!(registration_tokens::Service),
			rooms: rooms::Service {
				alias: build!(rooms::alias::Service),
				auth_chain: build!(rooms::auth_chain::Service),
				directory: build!(rooms::directory::Service),
				event_handler: build!(rooms::event_handler::Service),
				lazy_loading: build!(rooms::lazy_loading::Service),
				metadata: build!(rooms::metadata::Service),
				outlier: build!(rooms::outlier::Service),
				pdu_metadata: build!(rooms::pdu_metadata::Service),
				read_receipt: build!(rooms::read_receipt::Service),
				search: build!(rooms::search::Service),
				short: build!(rooms::short::Service),
				spaces: build!(rooms::spaces::Service),
				state: build!(rooms::state::Service),
				state_accessor: build!(rooms::state_accessor::Service),
				state_cache: build!(rooms::state_cache::Service),
				state_compressor: build!(rooms::state_compressor::Service),
				threads: build!(rooms::threads::Service),
				timeline: build!(rooms::timeline::Service),
				typing: build!(rooms::typing::Service),
				user: build!(rooms::user::Service),
			},
			federation: build!(federation::Service),
			sending: build!(sending::Service),
			server_keys: build!(server_keys::Service),
			sync: build!(sync::Service),
			transaction_ids: build!(transaction_ids::Service),
			uiaa: build!(uiaa::Service),
			users: build!(users::Service),
			moderation: build!(moderation::Service),
			announcements: build!(announcements::Service),
			antispam: build!(antispam::Service),

			manager: Mutex::new(None),
			service,
			server,
			db,
		}))
	}

	pub async fn start(self: &Arc<Self>) -> Result<Arc<Self>> {
		debug_info!("Starting services...");

		self.admin.set_services(Some(Arc::clone(self)).as_ref());
		super::migrations::migrations(self).await?;
		self.manager
			.lock()
			.await
			.insert(Manager::new(self))
			.clone()
			.start()
			.await?;

		// reset dormant online/away statuses to offline, and set the server user as
		// online
		if self.server.config.allow_local_presence {
			self.presence.unset_all_presence().await;
			_ = self
				.presence
				.ping_presence(&self.globals.server_user, &ruma::presence::PresenceState::Online)
				.await;
		}

		debug_info!("Services startup complete.");

		Ok(Arc::clone(self))
	}

	pub async fn stop(&self) {
		info!("Shutting down services...");

		// set the server user as offline
		if self.server.config.allow_local_presence {
			_ = self
				.presence
				.ping_presence(&self.globals.server_user, &ruma::presence::PresenceState::Offline)
				.await;
		}

		self.interrupt();
		if let Some(manager) = self.manager.lock().await.as_ref() {
			manager.stop().await;
		}

		self.admin.set_services(None);

		debug_info!("Services shutdown complete.");
	}

	pub async fn poll(&self) -> Result<()> {
		if let Some(manager) = self.manager.lock().await.as_ref() {
			return manager.poll().await;
		}

		Ok(())
	}

	pub async fn clear_cache(&self) {
		self.services()
			.for_each(|service| async move {
				service.clear_cache().await;
			})
			.await;
	}

	pub async fn memory_usage(&self) -> Result<String> {
		self.services()
			.map(Ok)
			.try_fold(String::new(), |mut out, service| async move {
				service.memory_usage(&mut out).await?;
				Ok(out)
			})
			.await
	}

	fn interrupt(&self) {
		debug!("Interrupting services...");
		for (name, (service, ..)) in self.service.read().iter() {
			if let Some(service) = service.upgrade() {
				trace!("Interrupting {name}");
				service.interrupt();
			}
		}
	}

	/// Iterate from snapshot of the services map
	fn services(&self) -> impl Stream<Item = Arc<dyn Service>> + Send {
		self.service
			.read()
			.values()
			.filter_map(|val| val.0.upgrade())
			.collect::<Vec<_>>()
			.into_iter()
			.stream()
	}

	#[inline]
	pub fn try_get<T>(&self, name: &str) -> Result<Arc<T>>
	where
		T: Any + Send + Sync + Sized,
	{
		service::try_get::<T>(&self.service, name)
	}

	#[inline]
	pub fn get<T>(&self, name: &str) -> Option<Arc<T>>
	where
		T: Any + Send + Sync + Sized,
	{
		service::get::<T>(&self.service, name)
	}
}

#[allow(clippy::needless_pass_by_value)]
fn add_service(map: &Arc<Map>, s: Arc<dyn Service>, a: Arc<dyn Any + Send + Sync>) {
	let name = s.name();
	let len = map.read().len();

	trace!("built service #{len}: {name:?}");
	map.write()
		.insert(name.to_owned(), (Arc::downgrade(&s), Arc::downgrade(&a)));
}
