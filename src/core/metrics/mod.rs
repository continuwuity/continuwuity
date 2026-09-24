mod instruments;
mod process;
mod tests;

use std::sync::atomic::AtomicU32;

use opentelemetry::metrics::Meter;
pub use opentelemetry::{KeyValue, metrics::AsyncInstrument};
pub use prometheus::TEXT_FORMAT as PROMETHEUS_CONTENT_TYPE;
use prometheus::{Encoder, TextEncoder};
use tokio::runtime;
use tokio_metrics::TaskMonitor;
#[cfg(tokio_unstable)]
use tokio_metrics::{RuntimeIntervals, RuntimeMonitor};

pub use self::instruments::{Instruments, method_label};
use crate::{Err, Result};

pub struct Metrics {
	_runtime: Option<runtime::Handle>,

	runtime_metrics: Option<runtime::RuntimeMetrics>,

	task_monitor: Option<TaskMonitor>,

	#[cfg(tokio_unstable)]
	_runtime_monitor: Option<RuntimeMonitor>,

	#[cfg(tokio_unstable)]
	runtime_intervals: std::sync::Mutex<Option<RuntimeIntervals>>,

	// TODO: move stats
	pub requests_handle_active: AtomicU32,
	pub requests_handle_finished: AtomicU32,
	pub requests_panic: AtomicU32,

	pub meter: Meter,
	pub instruments: Instruments,
	registry: Option<prometheus::Registry>,
}

impl Metrics {
	#[must_use]
	pub fn new(
		runtime: Option<runtime::Handle>,
		meter: Meter,
		registry: Option<prometheus::Registry>,
	) -> Self {
		#[cfg(tokio_unstable)]
		let runtime_monitor = runtime.as_ref().map(RuntimeMonitor::new);

		#[cfg(tokio_unstable)]
		let runtime_intervals = runtime_monitor.as_ref().map(RuntimeMonitor::intervals);

		let runtime_metrics = runtime.as_ref().map(runtime::Handle::metrics);

		Self {
			_runtime: runtime.clone(),

			instruments: Instruments::new(&meter, runtime_metrics.clone()),
			meter,
			registry,

			runtime_metrics,

			task_monitor: runtime.map(|_| TaskMonitor::new()),

			#[cfg(tokio_unstable)]
			_runtime_monitor: runtime_monitor,

			#[cfg(tokio_unstable)]
			runtime_intervals: std::sync::Mutex::new(runtime_intervals),

			requests_handle_active: AtomicU32::new(0),
			requests_handle_finished: AtomicU32::new(0),
			requests_panic: AtomicU32::new(0),
		}
	}

	#[cfg(tokio_unstable)]
	pub fn runtime_interval(&self) -> Option<tokio_metrics::RuntimeMetrics> {
		self.runtime_intervals
			.lock()
			.expect("locked")
			.as_mut()
			.map(Iterator::next)
			.expect("next interval")
	}

	pub fn render(&self) -> Result<String> {
		let Some(registry) = &self.registry else {
			return Err!("Prometheus metrics are not enabled");
		};

		let mut buf = Vec::new();
		TextEncoder::new()
			.encode(&registry.gather(), &mut buf)
			.map_err(|e| crate::err!("{e}"))?;

		Ok(String::from_utf8(buf)?)
	}

	#[inline]
	pub fn task_root(&self) -> Option<&TaskMonitor> { self.task_monitor.as_ref() }

	#[inline]
	pub fn num_workers(&self) -> usize {
		self.runtime_metrics()
			.map_or(0, runtime::RuntimeMetrics::num_workers)
	}

	#[inline]
	pub fn runtime_metrics(&self) -> Option<&runtime::RuntimeMetrics> {
		self.runtime_metrics.as_ref()
	}
}
