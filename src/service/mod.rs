#![type_length_limit = "8192"]
#![allow(refining_impl_trait)]

extern crate conduwuit_core as conduwuit;
extern crate conduwuit_database as database;
mod manager;
mod migrations;
mod service;
pub mod services;
pub mod state;

pub mod account_data;
pub mod admin;
pub mod announcements;
pub mod antispam;
pub mod appservice;
pub mod client;
pub mod config;
pub mod emergency;
pub mod federation;
pub mod globals;
pub mod key_backups;
pub mod media;
pub mod moderation;
pub mod presence;
pub mod pusher;
pub mod registration_tokens;
pub mod resolver;
pub mod rooms;
pub mod sending;
pub mod server_keys;
pub mod sync;
pub mod transaction_ids;
pub mod uiaa;
pub mod users;

pub(crate) use service::{Args, Dep, Service};

pub use crate::services::Services;

conduwuit::mod_ctor! {}
conduwuit::mod_dtor! {}
