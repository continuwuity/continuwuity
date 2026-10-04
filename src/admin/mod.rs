#![recursion_limit = "192"]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::enum_glob_use)]
#![allow(clippy::too_many_arguments)]

continuwuity_macros::introspect_crate! {}

pub(crate) mod admin;
pub(crate) mod context;
pub(crate) mod processor;
mod tests;
pub(crate) mod utils;

pub(crate) mod appservice;
pub(crate) mod check;
pub(crate) mod debug;
pub(crate) mod federation;
pub(crate) mod media;
pub(crate) mod oidc;
pub(crate) mod query;
pub(crate) mod room;
pub(crate) mod server;
pub(crate) mod token;
pub(crate) mod user;

extern crate continuwuity_api as api;
extern crate continuwuity_core as continuwuity;
extern crate continuwuity_service as service;

pub(crate) use continuwuity_macros::admin_command_dispatch;

pub(crate) use crate::{context::Context, utils::get_room_info};

pub(crate) const PAGE_SIZE: usize = 100;

continuwuity::mod_ctor! {}
continuwuity::mod_dtor! {}

pub use crate::admin::AdminCommand;

/// Install the admin command processor
pub async fn init(admin_service: &service::admin::Service) {
	_ = admin_service.complete.write().insert(processor::complete);
	_ = admin_service
		.handle
		.write()
		.await
		.insert(processor::dispatch);
}

/// Uninstall the admin command handler
pub async fn fini(admin_service: &service::admin::Service) {
	_ = admin_service.handle.write().await.take();
	_ = admin_service.complete.write().take();
}
