use conduwuit_core::{Result, config::Config, err, metrics::Metrics};
use opentelemetry::{KeyValue, metrics::MeterProvider};
use opentelemetry_sdk::{
	Resource,
	metrics::SdkMeterProvider,
	resource::{EnvResourceDetector, TelemetryResourceDetector},
};
use tokio::runtime;

/// The returned provider must outlive the server so pending OTLP exports are
/// flushed on drop.
pub(crate) fn init(
	config: &Config,
	runtime: Option<&runtime::Handle>,
) -> Result<(Metrics, SdkMeterProvider)> {
	// Defaults first so OTEL_RESOURCE_ATTRIBUTES and OTEL_SERVICE_NAME win.
	let mut resource = Resource::builder_empty()
		.with_detector(Box::new(TelemetryResourceDetector))
		.with_service_name(conduwuit_core::BRANDING)
		.with_attributes([
			KeyValue::new("service.version", conduwuit_core::version()),
			KeyValue::new("service.instance.id", config.server_name.to_string()),
		])
		.with_detector(Box::new(EnvResourceDetector::new()));
	if let Some(name) = std::env::var("OTEL_SERVICE_NAME")
		.ok()
		.filter(|name| !name.is_empty())
	{
		resource = resource.with_service_name(name);
	}
	let resource = resource.build();

	let mut builder = SdkMeterProvider::builder().with_resource(resource);

	let registry = config
		.allow_prometheus_metrics
		.then(prometheus::Registry::new);
	if let Some(registry) = &registry {
		let exporter = opentelemetry_prometheus::exporter()
			.with_registry(registry.clone())
			.scope_info_enabled(false)
			.build()
			.map_err(|e| err!(Config("allow_prometheus_metrics", "{e}")))?;

		builder = builder.with_reader(exporter);
	}

	#[cfg(feature = "otlp_telemetry")]
	if config.allow_otlp_metrics {
		use opentelemetry_otlp::{MetricExporter, Protocol, WithExportConfig};

		let exporter = match config.otlp_protocol.as_str() {
			| "grpc" => MetricExporter::builder()
				.with_tonic()
				.with_protocol(Protocol::Grpc)
				.build(),
			| _ => MetricExporter::builder().with_http().build(),
		}
		.map_err(|e| err!(Config("allow_otlp_metrics", "{e}")))?;

		builder = builder.with_periodic_exporter(exporter);
	}

	#[cfg(not(feature = "otlp_telemetry"))]
	if config.allow_otlp_metrics {
		conduwuit_core::warn!(
			"allow_otlp_metrics is set but this build does not include the otlp_telemetry \
			 feature"
		);
	}

	let provider = builder.build();
	let meter = provider.meter(conduwuit_core::BRANDING);

	Ok((Metrics::new(runtime.cloned(), meter, registry), provider))
}
