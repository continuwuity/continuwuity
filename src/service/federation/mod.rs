mod execute;

use std::sync::Arc;

use conduwuit::{Result, Server};

use crate::{Dep, client, moderation, resolver, server_keys};

pub struct Service {
	services: Services,
}

struct Services {
	server: Arc<Server>,
	client: Dep<client::Service>,
	resolver: Dep<resolver::Service>,
	server_keys: Dep<server_keys::Service>,
	moderation: Dep<moderation::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				server: args.server.clone(),
				client: args.depend::<client::Service>("client"),
				resolver: args.depend::<resolver::Service>("resolver"),
				server_keys: args.depend::<server_keys::Service>("server_keys"),
				moderation: args.depend::<moderation::Service>("moderation"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}
