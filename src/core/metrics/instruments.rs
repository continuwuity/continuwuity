use std::time::{Duration, Instant};

use opentelemetry::{
	KeyValue,
	metrics::{Counter, Histogram, Meter},
};
use opentelemetry_semantic_conventions::{attribute, metric};
use tokio::runtime;

use super::process;

pub struct Instruments {
	pub federation_outgoing_request_duration: Histogram<f64>,
	pub federation_incoming_transaction_duration: Histogram<f64>,
	pub federation_incoming_pdus: Counter<u64>,
	pub federation_incoming_pdu_duration: Histogram<f64>,
	pub federation_incoming_pdu_lag: Histogram<f64>,
	pub federation_incoming_edus: Counter<u64>,
	pub federation_incoming_edu_duration: Histogram<f64>,

	pub sender_transaction_duration: Histogram<f64>,
	pub sender_events: Counter<u64>,
	pub sender_events_queued: Counter<u64>,

	pub media_write_size: Histogram<u64>,
	pub media_read_size: Histogram<u64>,

	pub antispam_checks: Counter<u64>,
	pub invites: Counter<u64>,
}

impl Instruments {
	pub(super) fn new(meter: &Meter, runtime_metrics: Option<runtime::RuntimeMetrics>) -> Self {
		register_process(meter);
		register_allocator(meter);
		if let Some(runtime_metrics) = runtime_metrics {
			register_runtime(meter, runtime_metrics);
		}

		let duration = |name: &'static str, description: &'static str| {
			meter
				.f64_histogram(name)
				.with_description(description)
				.with_unit("s")
				.with_boundaries(duration_buckets())
				.build()
		};
		let size = |name: &'static str, description: &'static str| {
			meter
				.u64_histogram(name)
				.with_description(description)
				.with_unit("By")
				.with_boundaries(size_buckets())
				.build()
		};

		Self {
			federation_outgoing_request_duration: duration(
				"continuwuity.federation.outgoing.request.duration",
				"Duration of outgoing federation requests",
			),
			federation_incoming_transaction_duration: duration(
				"continuwuity.federation.incoming.transaction.duration",
				"Time taken to process incoming federation transactions",
			),
			federation_incoming_pdus: meter
				.u64_counter("continuwuity.federation.incoming.pdus")
				.with_description("Incoming federation PDUs by event type and outcome")
				.with_unit("{pdu}")
				.build(),
			federation_incoming_pdu_duration: duration(
				"continuwuity.federation.incoming.pdu.duration",
				"Time taken to handle an incoming federation PDU",
			),
			federation_incoming_pdu_lag: meter
				.f64_histogram("continuwuity.federation.incoming.pdu.lag")
				.with_description(
					"Time between an accepted incoming PDU's origin_server_ts and its arrival",
				)
				.with_unit("s")
				.with_boundaries(vec![
					0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 300.0, 900.0, 3600.0,
				])
				.build(),
			federation_incoming_edus: meter
				.u64_counter("continuwuity.federation.incoming.edus")
				.with_description("Incoming federation EDUs by type")
				.with_unit("{edu}")
				.build(),
			federation_incoming_edu_duration: duration(
				"continuwuity.federation.incoming.edu.duration",
				"Time taken to handle an incoming federation EDU",
			),

			sender_transaction_duration: duration(
				"continuwuity.sender.transaction.duration",
				"Duration of outgoing transactions by destination type and outcome",
			),
			sender_events: meter
				.u64_counter("continuwuity.sender.events")
				.with_description("PDUs and EDUs sent in outgoing transactions by outcome")
				.with_unit("{event}")
				.build(),
			sender_events_queued: meter
				.u64_counter("continuwuity.sender.events.queued")
				.with_description("PDUs and EDUs queued for sending")
				.with_unit("{event}")
				.build(),

			media_write_size: size(
				"continuwuity.media.write.size",
				"Size of media files written, from local uploads or cached remote media",
			),
			media_read_size: size("continuwuity.media.read.size", "Size of media files served"),

			antispam_checks: meter
				.u64_counter("continuwuity.antispam.checks")
				.with_description("Antispam service checks by check and outcome")
				.with_unit("{check}")
				.build(),
			invites: meter
				.u64_counter("continuwuity.invites")
				.with_description("Invites sent by local users or received over federation")
				.with_unit("{invite}")
				.build(),
		}
	}
}

#[must_use]
pub fn duration_buckets() -> Vec<f64> { (0..16).map(|i| 0.001 * f64::from(1_u32 << i)).collect() }

fn size_buckets() -> Vec<f64> { (0..11).map(|i| 1024.0 * f64::from(4_u32.pow(i))).collect() }

fn register_process(meter: &Meter) {
	let started = Instant::now();
	meter
		.f64_observable_gauge(metric::PROCESS_UPTIME)
		.with_description("Time since the server started")
		.with_unit("s")
		.with_callback(move |observer| {
			observer.observe(started.elapsed().as_secs_f64(), &[]);
		})
		.build();

	meter
		.f64_observable_counter(metric::PROCESS_CPU_TIME)
		.with_description("Total CPU time spent by the process")
		.with_unit("s")
		.with_callback(|observer| {
			if let Some((user, system)) = process::cpu_time() {
				observer
					.observe(user.as_secs_f64(), &[KeyValue::new(attribute::CPU_MODE, "user")]);
				observer.observe(system.as_secs_f64(), &[KeyValue::new(
					attribute::CPU_MODE,
					"system",
				)]);
			}
		})
		.build();

	meter
		.i64_observable_up_down_counter(metric::PROCESS_MEMORY_USAGE)
		.with_description("Resident memory of the process")
		.with_unit("By")
		.with_callback(|observer| {
			if let Some(resident) =
				process::memory().and_then(|(resident, _)| resident.try_into().ok())
			{
				observer.observe(resident, &[]);
			}
		})
		.build();

	meter
		.i64_observable_up_down_counter(metric::PROCESS_MEMORY_VIRTUAL)
		.with_description("Virtual memory of the process")
		.with_unit("By")
		.with_callback(|observer| {
			if let Some(virt) = process::memory().and_then(|(_, virt)| virt.try_into().ok()) {
				observer.observe(virt, &[]);
			}
		})
		.build();

	meter
		.i64_observable_up_down_counter(metric::PROCESS_UNIX_FILE_DESCRIPTOR_COUNT)
		.with_description("Number of file descriptors in use by the process")
		.with_unit("{file_descriptor}")
		.with_callback(|observer| {
			if let Some(fds) = process::open_fds().and_then(|fds| fds.try_into().ok()) {
				observer.observe(fds, &[]);
			}
		})
		.build();
}

fn register_allocator(meter: &Meter) {
	meter
		.u64_observable_gauge("continuwuity.jemalloc.memory")
		.with_description("jemalloc memory statistics, when built with jemalloc_stats")
		.with_unit("By")
		.with_callback(|observer| {
			for (kind, bytes) in crate::alloc::memory_usage_stats().into_iter().flatten() {
				if let Ok(bytes) = bytes.try_into() {
					observer.observe(bytes, &[KeyValue::new("kind", kind)]);
				}
			}
		})
		.build();
}

fn register_runtime(meter: &Meter, metrics: runtime::RuntimeMetrics) {
	let gauge = |name: &'static str,
	             description: &'static str,
	             read: fn(&runtime::RuntimeMetrics) -> usize| {
		let metrics = metrics.clone();
		meter
			.u64_observable_gauge(name)
			.with_description(description)
			.with_callback(move |observer| {
				if let Ok(value) = read(&metrics).try_into() {
					observer.observe(value, &[]);
				}
			})
			.build();
	};

	gauge(
		"continuwuity.tokio.workers",
		"Number of tokio runtime worker threads",
		runtime::RuntimeMetrics::num_workers,
	);
	gauge(
		"continuwuity.tokio.alive_tasks",
		"Number of alive tasks in the tokio runtime",
		runtime::RuntimeMetrics::num_alive_tasks,
	);
	gauge(
		"continuwuity.tokio.global_queue_depth",
		"Number of tasks in the tokio runtime global queue",
		runtime::RuntimeMetrics::global_queue_depth,
	);

	let busy_metrics = metrics.clone();
	meter
		.f64_observable_counter("continuwuity.tokio.worker.busy_time")
		.with_description("Total time tokio worker threads spent busy")
		.with_unit("s")
		.with_callback(move |observer| {
			let busy: Duration = (0..busy_metrics.num_workers())
				.map(|worker| busy_metrics.worker_total_busy_duration(worker))
				.sum();
			observer.observe(busy.as_secs_f64(), &[]);
		})
		.build();

	meter
		.u64_observable_counter("continuwuity.tokio.worker.parks")
		.with_description("Total number of times tokio worker threads parked")
		.with_unit("{park}")
		.with_callback(move |observer| {
			let parks = (0..metrics.num_workers())
				.map(|worker| metrics.worker_park_count(worker))
				.sum();
			observer.observe(parks, &[]);
		})
		.build();
}
