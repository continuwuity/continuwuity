//! mimalloc allocator

use std::{
	any::Any,
	ffi::{CStr, c_void},
	panic::{self, AssertUnwindSafe},
	ptr,
};

use arrayvec::ArrayVec;
use libmimalloc_sys as ffi;
use mimalloc::MiMalloc;
use serde::Deserialize;

#[global_allocator]
static MIMALLOC: MiMalloc = MiMalloc;

#[derive(Deserialize)]
struct StatCount {
	current: i64,
	peak: i64,
}

#[derive(Deserialize)]
struct Stats {
	reserved: StatCount,
	committed: StatCount,
	purged: i64,
	reset: i64,
}

trait OutputFnChar {
	type Char;
}

impl<C> OutputFnChar for Option<unsafe extern "C" fn(*const C, *mut c_void)> {
	type Char = C;
}

type MiChar = <ffi::mi_output_fun as OutputFnChar>::Char;

struct StatsPrintState {
	buf: String,
	max_len: usize,
	panic: Option<Box<dyn Any + Send>>,
}

pub(crate) fn metric() -> Option<(&'static str, &'static str)> {
	Some(("continuwuity.mimalloc.memory", "mimalloc memory statistics"))
}

pub fn trim<I: Into<Option<usize>>>(_: I) -> crate::Result {
	// SAFETY: `mi_collect` is thread-safe and takes a single boolean flag.
	unsafe { ffi::mi_collect(true) };

	Ok(())
}

fn read_stats() -> Option<Stats> {
	let stats = MiMalloc::stats_json().ok()?;
	let json = stats.to_str().ok()?;
	serde_json::from_str(json).ok()
}

fn non_neg_usize(value: i64) -> usize { usize::try_from(value.max(0)).unwrap_or(usize::MAX) }

fn mibs(input: usize) -> f64 {
	let whole = u32::try_from(input / (1024 * 1024)).unwrap_or(u32::MAX);
	let rem_kibs = u32::try_from((input / 1024) % 1024).unwrap_or_default();
	f64::from(whole) + f64::from(rem_kibs) / 1024.0
}

#[must_use]
pub fn memory_usage() -> Option<String> {
	let stats = read_stats()?;

	Some(format!(
		"reserved: {:.2} MiB\nreserved_peak: {:.2} MiB\ncommitted: {:.2} MiB\ncommitted_peak: \
		 {:.2} MiB\npurged_total: {:.2} MiB\nreset_total: {:.2} MiB\n",
		mibs(non_neg_usize(stats.reserved.current)),
		mibs(non_neg_usize(stats.reserved.peak)),
		mibs(non_neg_usize(stats.committed.current)),
		mibs(non_neg_usize(stats.committed.peak)),
		mibs(non_neg_usize(stats.purged)),
		mibs(non_neg_usize(stats.reset)),
	))
}

#[must_use]
pub fn memory_usage_stats() -> Option<ArrayVec<(&'static str, usize), 6>> {
	let stats = read_stats()?;
	let mut out = ArrayVec::new();
	out.extend([
		("reserved", non_neg_usize(stats.reserved.current)),
		("reserved_peak", non_neg_usize(stats.reserved.peak)),
		("committed", non_neg_usize(stats.committed.current)),
		("committed_peak", non_neg_usize(stats.committed.peak)),
	]);

	Some(out)
}

#[must_use]
pub fn memory_stats(opts: &str) -> Option<String> {
	const MAX_LENGTH: usize = 1_048_576;

	if opts.contains('\0') {
		return None;
	}

	if opts.contains('J') {
		let stats = MiMalloc::stats_json().ok()?;
		let json = String::from_utf8_lossy(stats.to_bytes());
		let end = json.floor_char_boundary(json.len().min(MAX_LENGTH));
		return json.get(..end).map(str::to_owned);
	}

	let mut state = StatsPrintState {
		buf: String::new(),
		max_len: MAX_LENGTH,
		panic: None,
	};
	let opaque = ptr::from_mut(&mut state).cast::<c_void>();

	// SAFETY: `state` remains live in this frame for the duration of
	// `mi_stats_print_out()`, which invokes `mi_stats_cb` synchronously.
	unsafe { ffi::mi_stats_print_out(Some(mi_stats_cb), opaque) };

	if let Some(payload) = state.panic {
		panic::resume_unwind(payload);
	}

	Some(state.buf)
}

unsafe extern "C" fn mi_stats_cb(msg: *const MiChar, arg: *mut c_void) {
	// SAFETY: `arg` points to `StatsPrintState` on the caller's stack.
	let Some(state) = (unsafe { arg.cast::<StatsPrintState>().as_mut() }) else {
		return;
	};

	if state.panic.is_some() || msg.is_null() || state.buf.len() >= state.max_len {
		return;
	}

	// SAFETY: mimalloc guarantees `msg` is a valid NUL-terminated C string.
	let msg = unsafe { CStr::from_ptr(msg.cast()) };

	if let Err(err) = panic::catch_unwind(AssertUnwindSafe(|| {
		let msg = String::from_utf8_lossy(msg.to_bytes());
		let remaining = state.max_len.saturating_sub(state.buf.len());
		if msg.len() <= remaining {
			state.buf.push_str(msg.as_ref());
		} else {
			if let Some(prefix) = msg.get(..msg.floor_char_boundary(remaining)) {
				state.buf.push_str(prefix);
			}
			state.max_len = state.buf.len();
		}
	})) {
		state.panic = Some(err);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn trim_succeeds() { trim(None).expect("trim(None)"); }

	#[test]
	fn non_neg_usize_clamps_negative_values() {
		assert_eq!(non_neg_usize(-1), 0, "expected -1 to clamp to 0");
		assert_eq!(non_neg_usize(i64::MIN), 0, "expected i64::MIN to clamp to 0");
		assert_eq!(non_neg_usize(0), 0, "expected 0 to remain 0");
		assert_eq!(non_neg_usize(4096), 4096, "expected positive value unchanged");
	}

	#[test]
	fn memory_usage_stats_reports_allocator_bytes() {
		let buf = std::hint::black_box(vec![0_u8; 64 * 1024]);
		let stats = memory_usage_stats().expect("memory_usage_stats");
		drop(buf);

		let keys: Vec<_> = stats.iter().map(|&(key, _)| key).collect();
		assert_eq!(
			keys,
			["reserved", "reserved_peak", "committed", "committed_peak"],
			"unexpected stat keys"
		);

		for (key, bytes) in stats {
			assert!(bytes > 0, "expected {key} > 0, got {bytes}");
		}
	}

	#[test]
	fn memory_usage_returns_allocator_summary() {
		let usage = memory_usage().expect("memory_usage");
		for expected in [
			"reserved:",
			"reserved_peak:",
			"committed:",
			"committed_peak:",
			"purged_total:",
			"reset_total:",
			"MiB",
		] {
			assert!(usage.contains(expected), "missing {expected:?} in usage:\n{usage}");
		}
		assert!(!usage.contains("current_rss:"), "unexpected current_rss in usage:\n{usage}");
		assert!(!usage.contains("peak_rss:"), "unexpected peak_rss in usage:\n{usage}");
	}

	#[test]
	fn memory_stats_supports_text_json_and_rejects_nul() {
		assert!(memory_stats("invalid\0opts").is_none(), "expected NUL opts to return None");

		for opts in ["", "abcdefghijklmnopqrstuvwxyz"] {
			let stats = memory_stats(opts).expect("memory_stats text");
			assert!(!stats.is_empty(), "expected non-empty stats for opts={opts:?}");
			assert!(!stats.starts_with('{'), "expected formatted text, got JSON:\n{stats}");
			assert!(stats.contains("reserved"), "missing reserved in stats:\n{stats}");
			assert!(stats.contains("committed"), "missing committed in stats:\n{stats}");
		}

		let json = memory_stats("J").expect("memory_stats JSON");
		assert!(json.starts_with('{'), "expected JSON object, got:\n{json}");
		let parsed: serde_json::Value =
			serde_json::from_str(&json).expect("expected valid JSON from memory_stats(\"J\")");
		assert!(parsed.get("committed").is_some(), "expected committed field in JSON");
	}
}
