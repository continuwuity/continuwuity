use std::{
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use conduwuit::{
	metrics::{AsyncInstrument, KeyValue},
	utils::millis_since_unix_epoch,
};
use database::MemoryUsage;

use crate::Services;

/// Both database memory gauges are collected in the same scrape, so the
/// statistics are shared between them for this long.
const MEMORY_USAGE_TTL: Duration = Duration::from_secs(1);

impl Services {
	pub(crate) fn register_metrics(self: &Arc<Self>) {
		self.gauge(
			"continuwuity.sender.queue.depth",
			"Messages waiting in the sender worker channels",
			"{message}",
			|services, observer| {
				observe(observer, services.sending.queue_depth(), &[]);
			},
		);

		self.gauge(
			"continuwuity.federation.incoming.transactions.active",
			"Incoming federation transactions currently being processed",
			"{transaction}",
			|services, observer| {
				observe(observer, services.transactions.txn_active_handle_count(), &[]);
			},
		);

		self.gauge(
			"continuwuity.users",
			"Estimated known users, local including deactivated",
			"{user}",
			|services, observer| {
				for (map, origin) in
					[("userid_password", "local"), ("remoteuserid_remoteuser", "remote")]
				{
					if let Some(keys) = estimate_keys(services, map) {
						observer.observe(keys, &[KeyValue::new("origin", origin)]);
					}
				}
			},
		);
		self.gauge(
			"continuwuity.media.files",
			"Estimated stored media files including thumbnails",
			"{file}",
			|services, observer| {
				if let Some(keys) = estimate_keys(services, "mediaid_file") {
					observer.observe(keys, &[]);
				}
			},
		);

		self.gauge(
			"continuwuity.federation.destinations",
			"Remote servers in retry backoff or with a stale destination cache entry",
			"{server}",
			|services, observer| {
				let now = millis_since_unix_epoch();
				let backoff = services
					.federation
					.remote_health
					.read()
					.values()
					.filter(|&&(_, next_retry)| now < next_retry)
					.count();
				let stale = services.federation.stale_destinations.read().len();

				observe(observer, backoff, &[KeyValue::new("state", "backoff")]);
				observe(observer, stale, &[KeyValue::new("state", "stale")]);
			},
		);

		let memory = Arc::new(MemoryUsageCache::default());
		let block_cache_memory = memory.clone();
		self.gauge(
			"continuwuity.database.memory",
			"RocksDB memory usage by component",
			"By",
			move |services, observer| {
				let Some(usage) = memory.get(services) else {
					return;
				};

				for (name, bytes) in [
					("mem_tables", usage.mem_table_total),
					("mem_tables_unflushed", usage.mem_table_unflushed),
					("table_readers", usage.mem_table_readers_total),
					("row_cache", usage.row_cache),
				] {
					observer.observe(bytes, &[KeyValue::new("name", name)]);
				}
			},
		);

		self.gauge(
			"continuwuity.database.block_cache",
			"RocksDB block cache usage by column family",
			"By",
			move |services, observer| {
				let Some(usage) = block_cache_memory.get(services) else {
					return;
				};

				for (column, bytes) in &usage.col_caches {
					observer.observe(*bytes, &[KeyValue::new("column", column.clone())]);
				}
			},
		);

		self.gauge(
			"continuwuity.database.size",
			"Total size of RocksDB SST files",
			"By",
			|services, observer| {
				let size = services
					.db
					.iter()
					.filter_map(|(_, map)| {
						map.property_integer(c"rocksdb.total-sst-files-size").ok()
					})
					.fold(0_u64, u64::saturating_add);

				observer.observe(size, &[]);
			},
		);

		self.gauge(
			"continuwuity.cache.entries",
			"Entries held in in-memory caches",
			"{entry}",
			|services, observer| {
				let (auth_chain, _) = services.rooms.auth_chain.get_cache_usage();
				let stateinfo = services.rooms.state_compressor.stateinfo_cache.lock().len();

				observe(observer, auth_chain, &[KeyValue::new("name", "auth_chain")]);
				observe(observer, stateinfo, &[KeyValue::new("name", "stateinfo")]);
			},
		);
	}

	fn gauge<F>(
		self: &Arc<Self>,
		name: &'static str,
		description: &'static str,
		unit: &'static str,
		callback: F,
	) where
		F: Fn(&Self, &dyn AsyncInstrument<u64>) + Send + Sync + 'static,
	{
		let services = Arc::downgrade(self);
		self.server
			.metrics
			.meter
			.u64_observable_gauge(name)
			.with_description(description)
			.with_unit(unit)
			.with_callback(move |observer| {
				if let Some(services) = services.upgrade() {
					callback(&services, observer);
				}
			})
			.build();
	}
}

#[derive(Default)]
struct MemoryUsageCache(Mutex<Option<(Instant, Arc<MemoryUsage>)>>);

impl MemoryUsageCache {
	fn get(&self, services: &Services) -> Option<Arc<MemoryUsage>> {
		let mut cached = self.0.lock().expect("locked");
		if let Some((at, usage)) = &*cached {
			if at.elapsed() < MEMORY_USAGE_TTL {
				return Some(usage.clone());
			}
		}

		let usage = Arc::new(services.db.db.memory_usage_stats().ok()?);
		*cached = Some((Instant::now(), usage.clone()));
		Some(usage)
	}
}

/// Cheap estimate from RocksDB properties, unlike iterating the column.
fn estimate_keys(services: &Services, map: &'static str) -> Option<u64> {
	services
		.db
		.get(map)
		.ok()?
		.property_integer(c"rocksdb.estimate-num-keys")
		.ok()
}

fn observe(observer: &dyn AsyncInstrument<u64>, value: usize, attributes: &[KeyValue]) {
	if let Ok(value) = value.try_into() {
		observer.observe(value, attributes);
	}
}
