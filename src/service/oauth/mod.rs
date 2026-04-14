use std::sync::Arc;

use base64::Engine;
use conduwuit::{Result, utils::hash::sha256};
use database::{Deserialized, Json, Map};

use crate::{Dep, config, oauth::client_metadata::ClientMetadata};

pub mod client_metadata;

pub struct Service {
	services: Services,
	db: Data,
}

struct Data {
	clientid_clientmetadata: Arc<Map>,
}

struct Services {
	config: Dep<config::Service>,
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

	async fn get_client_registration(&self, client_id: &str) -> Option<ClientMetadata> {
		self.db
			.clientid_clientmetadata
			.get(client_id)
			.await
			.deserialized()
			.ok()
	}
}
