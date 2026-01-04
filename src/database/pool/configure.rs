use std::{path::PathBuf, sync::Arc};

use conduwuit::{
	Server, debug, debug_info, expected, is_equal_to,
	utils::{
		math::usize_from_f64,
		result::LogDebugErr,
		stream,
		stream::{AMPLIFICATION_LIMIT, WIDTH_LIMIT},
		sys::{compute::is_core_available, storage},
	},
};

use super::{QUEUE_LIMIT, WORKER_LIMIT};

pub(super) fn configure(server: &Arc<Server>) -> (usize, Vec<usize>, Vec<usize>) {
	let config = &server.config;

	// This finds the block device and gathers all the properties we need.
	let path: PathBuf = config.database_path.clone();
	let device_name = storage::name_from_path(&path).log_debug_err().ok();
	let device_prop = storage::parallelism(&path);

	// The default worker count is masked-on if we didn't find better information.
	let default_worker_count = device_prop.mq.is_empty().then_some(config.db_pool_workers);

	// Determine the worker groupings. Each indice represents a hardware queue and
	// contains the number of workers which will service it.
	let worker_counts: Vec<_> = device_prop
		.mq
		.iter()
		.filter(|mq| mq.cpu_list.iter().copied().any(is_core_available))
		.map(|mq| {
			let shares = mq
				.cpu_list
				.iter()
				.filter(|&&id| is_core_available(id))
				.count()
				.max(1);

			let limit = config.db_pool_workers_limit.saturating_mul(shares);

			let limit = device_prop.nr_requests.map_or(limit, |nr| nr.min(limit));

			mq.nr_tags.unwrap_or(WORKER_LIMIT.0).min(limit)
		})
		.chain(default_worker_count)
		.collect();

	// Determine our software queue size for each hardware queue. This is the mpmc
	// between the tokio worker and the pool worker.
	let queue_sizes: Vec<_> = worker_counts
		.iter()
		.map(|worker_count| {
			worker_count
				.saturating_mul(config.db_pool_queue_mult)
				.clamp(QUEUE_LIMIT.0, QUEUE_LIMIT.1)
		})
		.collect();

	// Determine the CPU affinities of each hardware queue. Each indice is a cpu and
	// each value is the associated hardware queue. There is a little shiftiness
	// going on because cpu's which are not available to the process are filtered
	// out, similar to the worker_counts.
	let topology = device_prop
		.mq
		.iter()
		.fold(vec![0; 128], |mut topology, mq| {
			mq.cpu_list
				.iter()
				.filter(|&&id| is_core_available(id))
				.for_each(|&id| {
					topology[id] = mq.id;
				});

			topology
		});

	// Regardless of the capacity of all queues we establish some limit on the total
	// number of workers; this is hopefully hinted by nr_requests.
	let max_workers = device_prop
		.mq
		.iter()
		.filter_map(|mq| mq.nr_tags)
		.chain(default_worker_count)
		.fold(0_usize, usize::saturating_add)
		.clamp(WORKER_LIMIT.0, WORKER_LIMIT.1);

	// Determine the final worker count which we'll be spawning.
	let total_workers = worker_counts
		.iter()
		.sum::<usize>()
		.clamp(WORKER_LIMIT.0, max_workers);

	// After computing all of the above we can update the global automatic stream
	// width, hopefully with a better value tailored to this system.
	if config.stream_width_scale > 0.0 {
		let num_queues = queue_sizes.len().max(1);
		update_stream_width(server, num_queues, total_workers);
	}

	debug_info!(
		device_name = ?device_name
			.as_deref()
			.unwrap_or("None"),
		?worker_counts,
		?queue_sizes,
		%total_workers,
		stream_width = %stream::automatic_width(),
		"Frontend topology",
	);

	assert!(total_workers > 0, "some workers expected");
	assert!(!queue_sizes.is_empty(), "some queues expected");
	assert!(
		!queue_sizes.iter().copied().any(is_equal_to!(0)),
		"positive queue sizes expected"
	);

	(total_workers, queue_sizes, topology)
}

#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
fn update_stream_width(server: &Arc<Server>, num_queues: usize, total_workers: usize) {
	let config = &server.config;
	let scale: f64 = config.stream_width_scale.min(100.0).into();

	let req_width = expected!(total_workers / num_queues).next_multiple_of(2);
	let req_width = req_width as f64;
	let req_width = usize_from_f64(req_width * scale)
		.expect("failed to convert f64 to usize")
		.clamp(WIDTH_LIMIT.0, WIDTH_LIMIT.1);

	let req_amp = config.stream_amplification as f64;
	let req_amp = usize_from_f64(req_amp * scale)
		.expect("failed to convert f64 to usize")
		.clamp(AMPLIFICATION_LIMIT.0, AMPLIFICATION_LIMIT.1);

	let (old_width, new_width) = stream::set_width(req_width);
	let (old_amp, new_amp) = stream::set_amplification(req_amp);
	debug!(
		scale = %config.stream_width_scale,
		%num_queues,
		%req_width,
		%old_width,
		%new_width,
		%old_amp,
		%new_amp,
		"Updated global stream width"
	);
}
