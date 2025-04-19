use std::sync::Arc;

use conduwuit::{Result, Server, implement};
use ruma::ServerName;

pub struct Service {
	services: Services,
}

struct Services {
	pub server: Arc<Server>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services { server: args.server.clone() },
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

#[implement(Service)]
#[must_use]
pub fn is_remote_server_forbidden(&self, server_name: &ServerName) -> bool {
	// Forbidden if NOT (allowed is empty OR allowed contains server OR is self)
	// OR forbidden contains server
	self.services
		.server
		.config
		.forbidden_remote_server_names
		.is_match(server_name.host())
}

#[implement(Service)]
#[must_use]
pub fn is_remote_server_room_directory_forbidden(&self, server_name: &ServerName) -> bool {
	// Forbidden if NOT (allowed is empty OR allowed contains server OR is self)
	// OR forbidden contains server
	self.is_remote_server_forbidden(server_name)
		|| self
			.services
			.server
			.config
			.forbidden_remote_room_directory_server_names
			.is_match(server_name.host())
}

#[implement(Service)]
#[must_use]
pub fn is_remote_server_media_downloads_forbidden(&self, server_name: &ServerName) -> bool {
	// Forbidden if NOT (allowed is empty OR allowed contains server OR is self)
	// OR forbidden contains server
	self.is_remote_server_forbidden(server_name)
		|| self
			.services
			.server
			.config
			.prevent_media_downloads_from
			.is_match(server_name.host())
}
