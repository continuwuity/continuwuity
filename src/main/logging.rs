use std::sync::Arc;

use conduwuit_core::{
	Result,
	config::Config,
	debug_warn, err,
	log::{ConsoleFormat, ConsoleWriter, LogLevelReloadHandles, capture, fmt_span},
	result::UnwrapOrErr,
};
#[cfg(feature = "otlp_telemetry")]
use opentelemetry::trace::TracerProvider;
use tracing_subscriber::{EnvFilter, Layer, Registry, fmt, layer::SubscriberExt, reload};

#[cfg(feature = "perf_measurements")]
pub(crate) type TracingFlameGuard =
	Option<tracing_flame::FlushGuard<std::io::BufWriter<std::fs::File>>>;
#[cfg(not(feature = "perf_measurements"))]
pub(crate) type TracingFlameGuard = ();

#[allow(clippy::redundant_clone)]
pub(crate) fn init(
	config: &Config,
) -> Result<(LogLevelReloadHandles, TracingFlameGuard, Arc<capture::State>)> {
	let reload_handles = LogLevelReloadHandles::default();

	let console_span_events = fmt_span::from_str(&config.log_span_events).unwrap_or_err();

	let console_filter = EnvFilter::builder()
		.with_regex(config.log_filter_regex)
		.parse(&config.log)
		.map_err(|e| err!(Config("log", "{e}.")))?;

	let console_layer = fmt::Layer::new()
		.with_span_events(console_span_events)
		.event_format(ConsoleFormat::new(config))
		.fmt_fields(ConsoleFormat::new(config))
		.with_writer(ConsoleWriter::new(config));

	let (console_reload_filter, console_reload_handle) =
		reload::Layer::new(console_filter.clone());

	reload_handles.add("console", Box::new(console_reload_handle));

	let cap_state = Arc::new(capture::State::new());
	let cap_layer = capture::Layer::new(&cap_state);

	let subscriber = Registry::default()
		.with(console_layer.with_filter(console_reload_filter))
		.with(cap_layer);

	// If journald logging is enabled on Unix platforms, create a separate
	// subscriber for it
	#[cfg(all(target_family = "unix", feature = "journald"))]
	if config.log_to_journald {
		println!("Initialising journald logging");
		if let Err(e) = init_journald_logging(config) {
			eprintln!("Failed to initialize journald logging: {e}");
		}
	}

	#[cfg(feature = "sentry_telemetry")]
	let subscriber = {
		let sentry_filter = EnvFilter::try_new(&config.sentry_filter)
			.map_err(|e| err!(Config("sentry_filter", "{e}.")))?;

		let sentry_layer = sentry_tracing::layer();
		let (sentry_reload_filter, sentry_reload_handle) = reload::Layer::new(sentry_filter);

		reload_handles.add("sentry", Box::new(sentry_reload_handle));
		subscriber.with(sentry_layer.with_filter(sentry_reload_filter))
	};

	#[cfg(feature = "otlp_telemetry")]
	let subscriber = {
		let otlp_filter = EnvFilter::try_new(&config.otlp_filter)
			.map_err(|e| err!(Config("otlp_filter", "{e}.")))?;

		let otlp_layer = config.allow_otlp.then(|| {
			opentelemetry::global::set_text_map_propagator(
				opentelemetry_sdk::propagation::TraceContextPropagator::new(),
			);

			let exporter = opentelemetry_otlp::SpanExporter::builder()
				.with_http()
				.build()
				.expect("Failed to create OTLP exporter");

			let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
				.with_batch_exporter(exporter)
				.build();

			let tracer = provider.tracer(conduwuit_core::name());

			let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

			let (otlp_reload_filter, otlp_reload_handle) =
				reload::Layer::new(otlp_filter.clone());
			reload_handles.add("otlp", Box::new(otlp_reload_handle));

			Some(telemetry.with_filter(otlp_reload_filter))
		});

		subscriber.with(otlp_layer)
	};

	#[cfg(feature = "perf_measurements")]
	let (subscriber, flame_guard) = {
		let (flame_layer, flame_guard) = if config.tracing_flame {
			let flame_filter = EnvFilter::try_new(&config.tracing_flame_filter)
				.map_err(|e| err!(Config("tracing_flame_filter", "{e}.")))?;

			let (flame_layer, flame_guard) =
				tracing_flame::FlameLayer::with_file(&config.tracing_flame_output_path)
					.map_err(|e| err!(Config("tracing_flame_output_path", "{e}.")))?;

			let flame_layer = flame_layer
				.with_empty_samples(false)
				.with_filter(flame_filter);

			(Some(flame_layer), Some(flame_guard))
		} else {
			(None, None)
		};

		let subscriber = subscriber.with(flame_layer);
		(subscriber, flame_guard)
	};

	#[cfg(not(feature = "perf_measurements"))]
	#[cfg_attr(not(feature = "perf_measurements"), allow(clippy::let_unit_value))]
	let flame_guard = ();

	let ret = (reload_handles, flame_guard, cap_state);

	// Enable the tokio console. This is slightly kludgy because we're judggling
	// compile-time and runtime conditions to elide it, each of those changing the
	// subscriber's type.
	let (console_enabled, console_disabled_reason) = tokio_console_enabled(config);
	#[cfg(all(feature = "tokio_console", tokio_unstable))]
	if console_enabled {
		let console_layer = console_subscriber::ConsoleLayer::builder()
			.with_default_env()
			.spawn();

		set_global_default(subscriber.with(console_layer));
		return Ok(ret);
	}

	set_global_default(subscriber);

	// If there's a reason the tokio console was disabled when it might be desired
	// we output that here after initializing logging
	if !console_enabled && !console_disabled_reason.is_empty() {
		debug_warn!("{console_disabled_reason}");
	}

	Ok(ret)
}

#[cfg(all(target_family = "unix", feature = "journald"))]
fn init_journald_logging(config: &Config) -> Result<()> {
	use tracing_journald::Layer as JournaldLayer;

	let journald_filter =
		EnvFilter::try_new(&config.log).map_err(|e| err!(Config("log", "{e}.")))?;

	let mut journald_layer = JournaldLayer::new()
		.map_err(|e| err!(Config("journald", "Failed to initialize journald layer: {e}.")))?;

	if let Some(ref identifier) = config.journald_identifier {
		journald_layer = journald_layer.with_syslog_identifier(identifier.to_owned());
	}

	let journald_subscriber =
		Registry::default().with(journald_layer.with_filter(journald_filter));

	let _guard = tracing::subscriber::set_default(journald_subscriber);

	Ok(())
}

fn tokio_console_enabled(config: &Config) -> (bool, &'static str) {
	if !cfg!(all(feature = "tokio_console", tokio_unstable)) {
		return (false, "");
	}

	if cfg!(feature = "release_max_log_level") && !cfg!(debug_assertions) {
		return (
			false,
			"'tokio_console' feature and 'release_max_log_level' feature are incompatible.",
		);
	}

	if !config.tokio_console {
		return (false, "tokio console is available but disabled by the configuration.");
	}

	(true, "")
}

fn set_global_default<S>(subscriber: S)
where
	S: tracing::Subscriber + Send + Sync + 'static,
{
	tracing::subscriber::set_global_default(subscriber)
		.expect("the global default tracing subscriber failed to be initialized");
}
