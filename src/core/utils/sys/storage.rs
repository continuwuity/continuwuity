//! System utilities related to devices/peripherals

use std::{
	ffi::OsStr,
	fs,
	fs::{FileType, read_to_string},
	iter::IntoIterator,
	path::{Path, PathBuf},
};

use libc::dev_t;

use crate::{
	Result,
	result::FlatOk,
	utils::{result::LogDebugErr, string::SplitInfallible},
};

/// Device characteristics useful for random access throughput
#[derive(Clone, Debug, Default)]
pub struct Parallelism {
	/// Number of requests for the device.
	pub nr_requests: Option<usize>,

	/// Individual queue characteristics.
	pub mq: Vec<Queue>,
}

/// Device queue characteristics
#[derive(Clone, Debug, Default)]
pub struct Queue {
	/// Queue's indice.
	pub id: usize,

	/// Number of requests for the queue.
	pub nr_tags: Option<usize>,

	/// CPU affinities for the queue.
	pub cpu_list: Vec<usize>,
}

/// Get device characteristics useful for random access throughput by name.
#[must_use]
pub fn parallelism(path: &Path) -> Parallelism {
	let dev_id = dev_from_path(path).log_debug_err().unwrap_or_default();

	let mq_path = block_path(dev_id).join("mq/");

	let nr_requests_path = block_path(dev_id).join("queue/nr_requests");

	Parallelism {
		nr_requests: read_to_string(&nr_requests_path)
			.ok()
			.as_deref()
			.map(str::trim)
			.map(str::parse)
			.flat_ok(),

		mq: fs::read_dir(&mq_path)
			.into_iter()
			.flat_map(IntoIterator::into_iter)
			.filter_map(Result::ok)
			.filter(|entry| entry.file_type().as_ref().is_ok_and(FileType::is_dir))
			.map(|dir| queue_parallelism(&dir.path()))
			.collect(),
	}
}

/// Get device queue characteristics by mq path on sysfs(5)
fn queue_parallelism(dir: &Path) -> Queue {
	let queue_id = dir.file_name();

	let nr_tags_path = dir.join("nr_tags");

	let cpu_list_path = dir.join("cpu_list");

	Queue {
		id: queue_id
			.and_then(OsStr::to_str)
			.map(str::parse)
			.flat_ok()
			.expect("queue has some numerical identifier"),

		nr_tags: read_to_string(&nr_tags_path)
			.ok()
			.as_deref()
			.map(str::trim)
			.map(str::parse)
			.flat_ok(),

		cpu_list: read_to_string(&cpu_list_path)
			.iter()
			.flat_map(|list| list.trim().split(','))
			.map(str::trim)
			.map(str::parse)
			.filter_map(Result::ok)
			.collect(),
	}
}

/// Get the name of the block device on which Path is mounted.
pub fn name_from_path(path: &Path) -> Result<String> {
	use std::io::{Error, ErrorKind::NotFound};

	let (major, minor) = dev_from_path(path)?;
	let path = block_path((major, minor)).join("uevent");
	read_to_string(path)
		.iter()
		.map(String::as_str)
		.flat_map(str::lines)
		.map(|line| line.split_once_infallible("="))
		.find_map(|(key, val)| (key == "DEVNAME").then_some(val))
		.ok_or_else(|| Error::new(NotFound, "DEVNAME not found."))
		.map_err(Into::into)
		.map(Into::into)
}

/// Get the (major, minor) of the block device on which Path is mounted.
#[allow(clippy::useless_conversion, clippy::unnecessary_fallible_conversions)]
fn dev_from_path(path: &Path) -> Result<(dev_t, dev_t)> {
	#[cfg(target_family = "unix")]
	use std::os::unix::fs::MetadataExt;

	let stat = fs::metadata(path)?;
	let dev_id = stat.dev().try_into()?;
	let (major, minor) = (libc::major(dev_id), libc::minor(dev_id));

	Ok((major.try_into()?, minor.try_into()?))
}

fn block_path((major, minor): (dev_t, dev_t)) -> PathBuf {
	format!("/sys/dev/block/{major}:{minor}/").into()
}
