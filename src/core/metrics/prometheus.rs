use std::time::Duration;

use prometheus_client::{
	collector::Collector,
	encoding::{DescriptorEncoder, EncodeLabelSet, EncodeMetric, text},
	metrics::{
		counter::{ConstCounter, Counter},
		family::Family,
		gauge::{ConstGauge, Gauge},
		histogram::{Histogram, exponential_buckets},
		info::Info,
	},
	registry::{Registry, Unit},
};
use tokio::runtime;

use crate::Result;

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct HttpLabels {
	pub method: &'static str,
	pub route: String,
	pub status: u16,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct OutcomeLabels {
	pub outcome: &'static str,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct TransactionLabels {
	pub destination: &'static str,
	pub outcome: &'static str,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct NameLabels {
	pub name: &'static str,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq, EncodeLabelSet)]
pub struct ColumnLabels {
	pub column: String,
}

pub struct Prometheus {
	registry: Registry,

	pub http_requests_in_flight: Gauge,
	pub http_request_duration: Family<HttpLabels, Histogram>,

	pub federation_request_duration: Family<OutcomeLabels, Histogram>,
	pub federation_unhealthy_destinations: Gauge,
	pub federation_stale_destinations: Gauge,

	pub sender_transactions: Family<TransactionLabels, Counter>,
	pub sender_queue_depth: Gauge,

	pub database_memory: Family<NameLabels, Gauge>,
	pub database_block_cache: Family<ColumnLabels, Gauge>,
	pub cache_entries: Family<NameLabels, Gauge>,
}

impl Prometheus {
	pub(super) fn new(runtime_metrics: Option<runtime::RuntimeMetrics>) -> Self {
		let mut this = Self {
			registry: Registry::with_prefix("continuwuity"),
			http_requests_in_flight: Gauge::default(),
			http_request_duration: Family::new_with_constructor(duration_histogram),
			federation_request_duration: Family::new_with_constructor(duration_histogram),
			federation_unhealthy_destinations: Gauge::default(),
			federation_stale_destinations: Gauge::default(),
			sender_transactions: Family::default(),
			sender_queue_depth: Gauge::default(),
			database_memory: Family::default(),
			database_block_cache: Family::default(),
			cache_entries: Family::default(),
		};

		this.registry.register(
			"http_requests_in_flight",
			"Number of client and federation requests currently being handled",
			this.http_requests_in_flight.clone(),
		);
		this.registry.register_with_unit(
			"http_request_duration",
			"Duration of handled HTTP requests",
			Unit::Seconds,
			this.http_request_duration.clone(),
		);
		this.registry.register_with_unit(
			"federation_request_duration",
			"Duration of outgoing federation requests",
			Unit::Seconds,
			this.federation_request_duration.clone(),
		);
		this.registry.register(
			"federation_unhealthy_destinations",
			"Number of remote servers currently in retry backoff",
			this.federation_unhealthy_destinations.clone(),
		);
		this.registry.register(
			"federation_stale_destinations",
			"Number of remote servers with a stale destination cache entry",
			this.federation_stale_destinations.clone(),
		);
		this.registry.register(
			"sender_transactions",
			"Outgoing transactions completed by the sender, by destination type and outcome",
			this.sender_transactions.clone(),
		);
		this.registry.register(
			"sender_queue_depth",
			"Number of messages waiting in the sender worker channels",
			this.sender_queue_depth.clone(),
		);
		this.registry.register_with_unit(
			"database_memory",
			"RocksDB memory usage by component",
			Unit::Bytes,
			this.database_memory.clone(),
		);
		this.registry.register_with_unit(
			"database_block_cache",
			"RocksDB block cache usage by column family",
			Unit::Bytes,
			this.database_block_cache.clone(),
		);
		this.registry.register(
			"cache_entries",
			"Number of entries held in in-memory caches",
			this.cache_entries.clone(),
		);

		this.registry.register(
			"build",
			"Build information",
			Info::new([("version", crate::version())]),
		);

		if let Some(runtime_metrics) = runtime_metrics {
			this.registry
				.register_collector(Box::new(RuntimeCollector(runtime_metrics)));
		}

		this
	}

	pub fn encode(&self) -> Result<String> {
		let mut buf = String::new();
		text::encode(&mut buf, &self.registry)?;
		Ok(buf)
	}
}

/// Collapse non-standard methods so arbitrary client input cannot create new
/// label values.
#[must_use]
pub fn method_label(method: &http::Method) -> &'static str {
	match *method {
		| http::Method::GET => "GET",
		| http::Method::HEAD => "HEAD",
		| http::Method::POST => "POST",
		| http::Method::PUT => "PUT",
		| http::Method::DELETE => "DELETE",
		| http::Method::OPTIONS => "OPTIONS",
		| http::Method::PATCH => "PATCH",
		| _ => "OTHER",
	}
}

fn duration_histogram() -> Histogram { Histogram::new(exponential_buckets(0.005, 2.0, 14)) }

#[derive(Debug)]
struct RuntimeCollector(runtime::RuntimeMetrics);

impl Collector for RuntimeCollector {
	fn encode(&self, mut encoder: DescriptorEncoder<'_>) -> Result<(), std::fmt::Error> {
		let metrics = &self.0;
		let workers = metrics.num_workers();
		let busy: Duration = (0..workers)
			.map(|worker| metrics.worker_total_busy_duration(worker))
			.sum();
		let parks: u64 = (0..workers)
			.map(|worker| metrics.worker_park_count(worker))
			.sum();

		let gauges = [
			("tokio_workers", "Number of tokio runtime worker threads.", workers),
			(
				"tokio_alive_tasks",
				"Number of alive tasks in the tokio runtime.",
				metrics.num_alive_tasks(),
			),
			(
				"tokio_global_queue_depth",
				"Number of tasks in the tokio runtime global queue.",
				metrics.global_queue_depth(),
			),
		];
		for (name, help, value) in gauges {
			let gauge = ConstGauge::new(value);
			gauge.encode(encoder.encode_descriptor(name, help, None, gauge.metric_type())?)?;
		}

		let busy = ConstCounter::new(busy.as_secs_f64());
		busy.encode(encoder.encode_descriptor(
			"tokio_worker_busy",
			"Total time tokio worker threads spent busy.",
			Some(&Unit::Seconds),
			busy.metric_type(),
		)?)?;

		let parks = ConstCounter::new(parks);
		parks.encode(encoder.encode_descriptor(
			"tokio_worker_parks",
			"Total number of times tokio worker threads parked.",
			None,
			parks.metric_type(),
		)?)?;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::{HttpLabels, Prometheus, method_label};

	#[tokio::test]
	async fn encodes_openmetrics() {
		let prometheus = Prometheus::new(Some(tokio::runtime::Handle::current().metrics()));
		prometheus
			.http_request_duration
			.get_or_create(&HttpLabels {
				method: method_label(&http::Method::GET),
				route: "/_matrix/client/versions".to_owned(),
				status: 200,
			})
			.observe(0.01);

		let body = prometheus.encode().unwrap();
		for name in [
			"continuwuity_http_request_duration_seconds_count{method=\"GET\",route=\"/_matrix/\
			 client/versions\",status=\"200\"} 1",
			"continuwuity_http_requests_in_flight 0",
			"continuwuity_build_info{version=",
			"# TYPE continuwuity_tokio_worker_busy_seconds counter",
		] {
			assert!(body.contains(name), "missing {name:?} in:\n{body}");
		}
		assert!(body.ends_with("# EOF\n"));
	}

	#[test]
	fn method_label_is_bounded() {
		let custom = http::Method::from_bytes(b"BREW").unwrap();
		assert_eq!(method_label(&custom), "OTHER");
	}
}
