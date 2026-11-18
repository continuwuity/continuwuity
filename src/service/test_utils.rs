use std::{path::PathBuf, sync::Arc};

use conduwuit::{
	Config, Server,
	log::{Log, LogLevelReloadHandles},
	metrics::Metrics,
};
use serde_json::json;
use tokio::sync::{Mutex, MutexGuard};

/// RocksDB's default `Env` is shared by the whole process and closing a
/// database stops its background threads, so tests must not keep databases
/// open in parallel. Hold the returned guard until the services are dropped.
static DATABASE: Mutex<()> = Mutex::const_new(());

type TestServices = (Arc<crate::Services>, PathBuf, MutexGuard<'static, ()>);

pub(crate) async fn test_services() -> TestServices { test_services_with(json!({})).await }

pub(crate) async fn test_services_with(overrides: serde_json::Value) -> TestServices {
	let guard = DATABASE.lock().await;
	let _ = rustls::crypto::ring::default_provider().install_default();
	let path = std::env::temp_dir()
		.join(format!("continuwuity-shared-state-{:016x}", rand::random::<u64>()));
	std::fs::create_dir(&path).unwrap();
	let mut config = json!({
		"server_name": "test.example",
		"database_path": path,
		"db_cache_capacity_mb": 16.0,
		"rocksdb_write_buffer_capacity_mb": 4.0,
		"allow_local_presence": false,
		"dns_servers": ["127.0.0.1"],
		"allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure": true
	});
	config
		.as_object_mut()
		.unwrap()
		.extend(overrides.as_object().unwrap().clone());
	let config: Config = serde_json::from_value(config).unwrap();
	let runtime = tokio::runtime::Handle::current();
	let server = Arc::new(Server::new(
		config,
		Some(runtime.clone()),
		Log {
			reload: LogLevelReloadHandles::default(),
			capture: Arc::default(),
		},
		Metrics::new(Some(runtime), opentelemetry::global::meter("test"), None),
	));
	let services = crate::Services::build(server).await.unwrap();
	(services, path, guard)
}
