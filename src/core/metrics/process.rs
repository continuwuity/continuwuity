use std::time::Duration;

/// User and system CPU time.
#[cfg(unix)]
pub(super) fn cpu_time() -> Option<(Duration, Duration)> {
	use nix::sys::resource::{UsageWho, getrusage};

	let usage = getrusage(UsageWho::RUSAGE_SELF).ok()?;
	let to_duration = |tv: nix::sys::time::TimeVal| {
		Duration::from_secs(tv.tv_sec().try_into().ok()?)
			.checked_add(Duration::from_micros(tv.tv_usec().try_into().ok()?))
	};

	Some((to_duration(usage.user_time())?, to_duration(usage.system_time())?))
}

#[cfg(not(unix))]
pub(super) fn cpu_time() -> Option<(Duration, Duration)> { None }

/// Resident and virtual memory in bytes, read from `/proc/self/status`.
#[cfg(target_os = "linux")]
pub(super) fn memory() -> Option<(u64, u64)> {
	let status = std::fs::read_to_string("/proc/self/status").ok()?;
	let field = |name: &str| -> Option<u64> {
		let value = status.lines().find_map(|line| line.strip_prefix(name))?;
		let kib: u64 = value.trim().trim_end_matches(" kB").parse().ok()?;
		kib.checked_mul(1024)
	};

	Some((field("VmRSS:")?, field("VmSize:")?))
}

#[cfg(not(target_os = "linux"))]
pub(super) fn memory() -> Option<(u64, u64)> { None }

#[cfg(target_os = "linux")]
pub(super) fn open_fds() -> Option<u64> {
	// The directory handle used for the listing is itself an open descriptor.
	let count = std::fs::read_dir("/proc/self/fd")
		.ok()?
		.count()
		.checked_sub(1)?;
	count.try_into().ok()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn open_fds() -> Option<u64> { None }
