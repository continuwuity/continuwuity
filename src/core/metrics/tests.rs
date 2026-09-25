#![cfg(test)]

use opentelemetry::metrics::MeterProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;

use super::{KeyValue, Metrics};

fn metrics() -> (Metrics, SdkMeterProvider) {
	let registry = prometheus::Registry::new();
	let exporter = opentelemetry_prometheus::exporter()
		.with_registry(registry.clone())
		.scope_info_enabled(false)
		.build()
		.unwrap();
	let provider = SdkMeterProvider::builder().with_reader(exporter).build();
	let meter = provider.meter("continuwuity");
	let handle = tokio::runtime::Handle::current();

	(Metrics::new(Some(handle), meter, Some(registry)), provider)
}

#[tokio::test]
async fn renders_prometheus_names() {
	let (metrics, _provider) = metrics();
	metrics
		.instruments
		.sender_events
		.add(3, &[KeyValue::new("destination", "federation"), KeyValue::new("kind", "pdu")]);

	let body = metrics.render().unwrap();
	for expected in [
		"continuwuity_sender_events_total{destination=\"federation\",kind=\"pdu\"} 3",
		"# TYPE process_uptime_seconds gauge",
		"process_cpu_time_seconds_total{cpu_mode=\"user\"}",
		"# TYPE continuwuity_tokio_worker_busy_time_seconds_total counter",
	] {
		assert!(body.contains(expected), "missing {expected:?} in:\n{body}");
	}
	assert!(!body.contains("otel_scope_name"));
}
