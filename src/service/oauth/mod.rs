use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::{Duration, SystemTime},
};

use base64::Engine;
use conduwuit::{Result, utils::hash::sha256};
use database::{Deserialized, Json, Map};
use ruma::{DeviceId, OwnedUserId, UserId};

use crate::{Dep, config, oauth::client_metadata::ClientMetadata};

pub mod client_metadata;

pub struct Service {
	services: Services,
	db: Data,
	tickets: Mutex<HashMap<String, HashMap<OAuthTicket, SystemTime>>>,
}

struct Data {
	clientid_clientmetadata: Arc<Map>,
}

struct Services {
	config: Dep<config::Service>,
}

/// A time-limited grant for a client to perform some sensitive action.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OAuthTicket {
	CrossSigningReset,
}

impl OAuthTicket {
	const MAX_AGE: Duration = Duration::from_mins(10);

	pub fn ticket_issue_path(&self) -> &'static str {
		match self {
			| Self::CrossSigningReset => "/account/cross_signing_reset",
		}
	}
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				config: args.depend::<config::Service>("config"),
			},
			db: Data {
				clientid_clientmetadata: args.db["clientid_clientmetadata"].clone(),
			},
			tickets: Mutex::default(),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	pub async fn register_client(
		&self,
		metadata: &ClientMetadata,
	) -> Result<String, &'static str> {
		metadata.validate()?;

		let client_id = base64::prelude::BASE64_STANDARD
			.encode(sha256::hash(serde_json::to_string(metadata).unwrap().as_bytes()));

		if self
			.db
			.clientid_clientmetadata
			.exists(&client_id)
			.await
			.is_err()
		{
			self.db
				.clientid_clientmetadata
				.raw_put(&client_id, Json(metadata.clone()));
		}

		Ok(client_id)
	}

	pub async fn get_client_registration(&self, client_id: &str) -> Option<ClientMetadata> {
		self.db
			.clientid_clientmetadata
			.get(client_id)
			.await
			.deserialized()
			.ok()
	}

	pub async fn get_client_id_for_device(&self, _device_id: &DeviceId) -> Option<String> {
		None // TODO
	}

	/// Issue a ticket for `localpart` to perform some action.
	pub fn issue_ticket(&self, localpart: String, ticket: OAuthTicket) {
		self.tickets
			.lock()
			.expect("should be able to lock tickets")
			.entry(localpart)
			.or_default()
			.insert(ticket, SystemTime::now());
	}

	/// Try to consume an unexpired ticket for `localpart`.
	pub fn try_consume_ticket(&self, localpart: &str, ticket: OAuthTicket) -> bool {
		let now = SystemTime::now();

		self.tickets
			.lock()
			.expect("should be able to lock tickets")
			.get_mut(localpart)
			.and_then(|tickets| tickets.remove(&ticket))
			.is_some_and(|issued| {
				now.duration_since(issued)
					.is_ok_and(|duration| duration < OAuthTicket::MAX_AGE)
			})
	}
}
