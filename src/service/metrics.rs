use std::{
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use conduwuit::{
	metrics::{AsyncInstrument, KeyValue},
	utils::millis_since_unix_epoch,
};
use futures::StreamExt;

use crate::Services;

const COUNT_INTERVAL: Duration = Duration::from_mins(1);

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

		let counts = self.spawn_counter();
		let users = counts.clone();
		self.gauge(
			"continuwuity.users",
			"Known users, local including deactivated, refreshed every minute",
			"{user}",
			move |_, observer| {
				let local = users.local_users.load(Ordering::Relaxed);
				let remote = users.remote_users.load(Ordering::Relaxed);
				observer.observe(local, &[KeyValue::new("origin", "local")]);
				observer.observe(remote, &[KeyValue::new("origin", "remote")]);
			},
		);
		self.gauge(
			"continuwuity.media.files",
			"Stored media files including thumbnails, refreshed every minute",
			"{file}",
			move |_, observer| {
				observer.observe(counts.media_files.load(Ordering::Relaxed), &[]);
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

		self.gauge(
			"continuwuity.database.memory",
			"RocksDB memory usage by component",
			"By",
			|services, observer| {
				let Ok(usage) = services.db.db.memory_usage_stats() else {
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
			|services, observer| {
				let Ok(usage) = services.db.db.memory_usage_stats() else {
					return;
				};

				for (column, bytes) in usage.col_caches {
					observer.observe(bytes, &[KeyValue::new("column", column)]);
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

	/// Counting requires iterating the database, so it runs in the background
	/// instead of in the collection callback.
	fn spawn_counter(self: &Arc<Self>) -> Arc<Counts> {
		let counts = Arc::new(Counts::default());
		let services = Arc::downgrade(self);
		let task_counts = counts.clone();
		self.server.runtime().spawn(async move {
			let mut interval = tokio::time::interval(COUNT_INTERVAL);
			loop {
				interval.tick().await;
				let Some(services) = services.upgrade() else {
					break;
				};
				if !services.server.running() {
					break;
				}

				let local = services.users.count().await;
				let remote = services.users.stream_remote_users().count().await;
				let media = match services.db.get("mediaid_file") {
					| Ok(map) => map.count().await,
					| Err(_) => 0,
				};

				for (count, value) in [
					(&task_counts.local_users, local),
					(&task_counts.remote_users, remote),
					(&task_counts.media_files, media),
				] {
					count.store(value.try_into().unwrap_or(u64::MAX), Ordering::Relaxed);
				}
			}
		});

		counts
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
struct Counts {
	local_users: AtomicU64,
	remote_users: AtomicU64,
	media_files: AtomicU64,
}

fn observe(observer: &dyn AsyncInstrument<u64>, value: usize, attributes: &[KeyValue]) {
	if let Ok(value) = value.try_into() {
		observer.observe(value, attributes);
	}
}
