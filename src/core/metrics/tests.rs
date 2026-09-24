#![cfg(test)]

use opentelemetry::metrics::MeterProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;

use super::{KeyValue, Metrics, method_label};

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
	let instruments = &metrics.instruments;
	instruments.http_server_request_duration.record(0.01, &[
		KeyValue::new("http.request.method", method_label(&http::Method::GET)),
		KeyValue::new("http.route", "/_matrix/client/versions"),
		KeyValue::new("http.response.status_code", 200_i64),
	]);
	instruments
		.sender_events
		.add(3, &[KeyValue::new("destination", "federation"), KeyValue::new("kind", "pdu")]);

	let body = metrics.render().unwrap();
	for expected in [
		"http_server_request_duration_seconds_count{http_request_method=\"GET\",\
		 http_response_status_code=\"200\",http_route=\"/_matrix/client/versions\"} 1",
		"continuwuity_sender_events_total{destination=\"federation\",kind=\"pdu\"} 3",
		"# TYPE process_uptime_seconds gauge",
		"process_cpu_time_seconds_total{cpu_mode=\"user\"}",
		"# TYPE continuwuity_tokio_worker_busy_time_seconds_total counter",
	] {
		assert!(body.contains(expected), "missing {expected:?} in:\n{body}");
	}
	assert!(!body.contains("otel_scope_name"));
}

#[test]
fn method_label_is_bounded() {
	let method = |name: &[u8]| http::Method::from_bytes(name).unwrap();
	assert_eq!(method_label(&method(b"QUERY")), "QUERY");
	assert_eq!(method_label(&method(b"BREW")), "_OTHER");
}
