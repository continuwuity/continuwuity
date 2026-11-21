//! mimalloc allocator

use std::ptr;

use libmimalloc_sys as ffi;
use mimalloc::MiMalloc;

#[global_allocator]
static MIMALLOC: MiMalloc = MiMalloc;

pub fn trim<I: Into<Option<usize>>>(_: I) -> crate::Result {
	// SAFETY: `mi_collect` is thread-safe and takes a single boolean flag.
	unsafe {
		ffi::mi_collect(true);
	}

	Ok(())
}

#[must_use]
pub fn memory_usage() -> Option<String> {
	let mut current_rss = 0;
	let mut peak_rss = 0;
	let mut current_commit = 0;
	let mut peak_commit = 0;

	// SAFETY: All non-null out-pointers are valid writable `usize` stack slots;
	// null pointers are explicitly permitted by `mi_process_info`.
	unsafe {
		ffi::mi_process_info(
			ptr::null_mut(),
			ptr::null_mut(),
			ptr::null_mut(),
			&raw mut current_rss,
			&raw mut peak_rss,
			&raw mut current_commit,
			&raw mut peak_commit,
			ptr::null_mut(),
		);
	}

	let mibs = |input: usize| {
		let whole = u32::try_from(input / (1024 * 1024)).unwrap_or(u32::MAX);
		let rem_kibs = u32::try_from((input / 1024) % 1024).unwrap_or_default();
		f64::from(whole) + f64::from(rem_kibs) / 1024.0
	};

	Some(format!(
		"current_rss: {:.2} MiB\npeak_rss: {:.2} MiB\ncurrent_commit: {:.2} MiB\npeak_commit: \
		 {:.2} MiB\n",
		mibs(current_rss),
		mibs(peak_rss),
		mibs(current_commit),
		mibs(peak_commit),
	))
}

#[must_use]
pub fn memory_usage_stats() -> Option<[(&'static str, usize); 6]> { None }

#[must_use]
pub fn memory_stats(_opts: &str) -> Option<String> {
	MiMalloc::stats_json()
		.ok()
		.and_then(|stats| stats.to_str().ok().map(str::to_owned))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn trim_succeeds() { trim(None).expect("trim(None)"); }

	#[test]
	fn memory_usage_returns_rss_and_commit() {
		let usage = memory_usage().expect("memory_usage");
		assert!(usage.contains("current_rss:"), "unexpected usage: {usage}");
		assert!(usage.contains("current_commit:"), "unexpected usage: {usage}");
	}

	#[test]
	fn memory_stats_returns_json() {
		let stats = memory_stats("").expect("memory_stats");
		assert!(stats.starts_with('{'), "expected JSON object, got: {stats}");
	}
}
