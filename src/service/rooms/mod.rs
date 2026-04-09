pub mod alias;
pub mod auth_chain;
pub mod directory;
pub mod event_handler;
pub mod lazy_loading;
pub mod metadata;
pub mod outlier;
pub mod pdu_metadata;
pub mod read_receipt;
pub mod search;
pub mod short;
pub mod state;
pub mod state_accessor;
pub mod state_cache;
pub mod state_compressor;
pub mod summary;
pub mod threads;
pub mod timeline;
pub mod typing;
pub mod user;

use std::sync::Arc;

pub struct Service {
	pub alias: Arc<alias::Service>,
	pub auth_chain: Arc<auth_chain::Service>,
	pub directory: Arc<directory::Service>,
	pub event_handler: Arc<event_handler::Service>,
	pub lazy_loading: Arc<lazy_loading::Service>,
	pub metadata: Arc<metadata::Service>,
	pub outlier: Arc<outlier::Service>,
	pub pdu_metadata: Arc<pdu_metadata::Service>,
	pub read_receipt: Arc<read_receipt::Service>,
	pub search: Arc<search::Service>,
	pub short: Arc<short::Service>,
	pub state: Arc<state::Service>,
	pub state_accessor: Arc<state_accessor::Service>,
	pub state_cache: Arc<state_cache::Service>,
	pub state_compressor: Arc<state_compressor::Service>,
	pub summary: Arc<summary::Service>,
	pub threads: Arc<threads::Service>,
	pub timeline: Arc<timeline::Service>,
	pub typing: Arc<typing::Service>,
	pub user: Arc<user::Service>,
}
