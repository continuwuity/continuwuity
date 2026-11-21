//! jemalloc allocator

use std::{
	any::Any,
	ffi::{CStr, CString, c_char, c_uint, c_void},
	fmt::Debug,
	panic::{self, AssertUnwindSafe},
	ptr,
	sync::OnceLock,
};

use arrayvec::ArrayVec;
use tikv_jemalloc_ctl as mallctl;
use tikv_jemalloc_sys as ffi;
use tikv_jemallocator as jemalloc;

use crate::{
	Result, err, is_equal_to, is_nonzero,
	utils::{math, math::Tried},
};

#[cfg(feature = "jemalloc_conf")]
#[repr(transparent)]
struct MallocConf(*const c_char);

// SAFETY: MallocConf points to static immutable NUL-terminated bytes.
#[cfg(feature = "jemalloc_conf")]
unsafe impl Sync for MallocConf {}

#[cfg(feature = "jemalloc_conf")]
const MALLOC_CONF_BYTES: &[u8] = const_str::concat_bytes!(
	"lg_extent_max_active_fit:4",
	",oversize_threshold:16777216",
	",tcache_max:2097152",
	",dirty_decay_ms:16000",
	",muzzy_decay_ms:144000",
	MALLOC_CONF_PLATFORM,
	MALLOC_CONF_PROF,
	0
);

#[cfg(all(feature = "jemalloc_conf", target_os = "linux"))]
const MALLOC_CONF_PLATFORM: &str = ",percpu_arena:percpu,metadata_thp:always,background_thread:true";
#[cfg(all(feature = "jemalloc_conf", not(target_os = "linux")))]
const MALLOC_CONF_PLATFORM: &str = "";

// Targets match `NO_UNPREFIXED_MALLOC_TARGETS` in `tikv-jemalloc-sys` when
// `unprefixed_malloc_on_supported_platforms` is enabled.
#[cfg(feature = "jemalloc_conf")]
#[used]
#[allow(non_upper_case_globals)]
#[cfg_attr(
	any(
		target_vendor = "apple",
		target_env = "musl",
		target_os = "android",
		target_os = "dragonfly",
	),
	unsafe(export_name = "_rjem_malloc_conf")
)]
#[cfg_attr(
	not(any(
		target_vendor = "apple",
		target_env = "musl",
		target_os = "android",
		target_os = "dragonfly",
	)),
	unsafe(no_mangle)
)]
static malloc_conf: MallocConf = MallocConf(MALLOC_CONF_BYTES.as_ptr().cast());

#[cfg(all(feature = "jemalloc_conf", feature = "jemalloc_prof"))]
const MALLOC_CONF_PROF: &str = ",prof_active:false";
#[cfg(all(feature = "jemalloc_conf", not(feature = "jemalloc_prof")))]
const MALLOC_CONF_PROF: &str = "";

#[global_allocator]
static JEMALLOC: jemalloc::Jemalloc = jemalloc::Jemalloc;

type Name = ArrayVec<u8, NAME_MAX>;
type Key = ArrayVec<usize, KEY_SEGS>;

const NAME_MAX: usize = 128;
const KEY_SEGS: usize = 8;
const ARENA_LIMIT: usize = 4095;
const ARENAS_ALL: usize = 4096;

#[ctor::ctor(unsafe)]
fn _static_initialization() {
	let mut epoch: u64 = 0;
	let mut len = size_of::<u64>();
	// SAFETY: Pre-initialize jemalloc before main() without touching Rust TLS
	// or unwinding across the constructor boundary.
	unsafe {
		ffi::mallctl(
			c"epoch".as_ptr(),
			ptr::from_mut(&mut epoch).cast::<c_void>(),
			&raw mut len,
			ptr::null_mut(),
			0,
		);
	}
}

#[must_use]
#[cfg(feature = "jemalloc_stats")]
pub fn memory_usage() -> Option<String> {
	let mibs = |input: usize| {
		let whole = u32::try_from(input / (1024 * 1024)).unwrap_or(u32::MAX);
		let rem_kibs = u32::try_from((input / 1024) % 1024).unwrap_or_default();
		f64::from(whole) + f64::from(rem_kibs) / 1024.0
	};

	let [allocated, active, mapped, metadata, resident, retained] =
		memory_usage_stats()?.map(|(_, bytes)| mibs(bytes));
	Some(format!(
		"allocated: {allocated:.2} MiB\nactive: {active:.2} MiB\nmapped: {mapped:.2} \
		 MiB\nmetadata: {metadata:.2} MiB\nresident: {resident:.2} MiB\nretained: {retained:.2} \
		 MiB\n"
	))
}

#[must_use]
#[cfg(not(feature = "jemalloc_stats"))]
pub fn memory_usage() -> Option<String> { None }

#[must_use]
#[cfg(feature = "jemalloc_stats")]
pub fn memory_usage_stats() -> Option<[(&'static str, usize); 6]> {
	use mallctl::stats;

	// Advance the epoch; ensure latest stats are pulled in
	inc_epoch().ok()?;

	Some([
		("allocated", stats::allocated::read().unwrap_or_default()),
		("active", stats::active::read().unwrap_or_default()),
		("mapped", stats::mapped::read().unwrap_or_default()),
		("metadata", stats::metadata::read().unwrap_or_default()),
		("resident", stats::resident::read().unwrap_or_default()),
		("retained", stats::retained::read().unwrap_or_default()),
	])
}

#[must_use]
#[cfg(not(feature = "jemalloc_stats"))]
pub fn memory_usage_stats() -> Option<[(&'static str, usize); 6]> { None }

struct StatsPrintState {
	buf: String,
	max_len: usize,
	panic: Option<Box<dyn Any + Send>>,
}

pub fn memory_stats(opts: &str) -> Option<String> {
	const MAX_LENGTH: usize = 1_048_576;

	let opts = CString::new(opts).ok()?;

	let mut state = StatsPrintState {
		buf: String::new(),
		max_len: MAX_LENGTH,
		panic: None,
	};
	let opaque = ptr::from_mut(&mut state).cast::<c_void>();

	// SAFETY: `state` and `opts` remain live in this frame for the duration of
	// `malloc_stats_print()`, which invokes `malloc_stats_cb` synchronously.
	unsafe { ffi::malloc_stats_print(Some(malloc_stats_cb), opaque, opts.as_ptr()) };

	if let Some(payload) = state.panic {
		panic::resume_unwind(payload);
	}

	Some(state.buf)
}

unsafe extern "C" fn malloc_stats_cb(opaque: *mut c_void, msg: *const c_char) {
	// SAFETY: `opaque` points to `StatsPrintState` on the caller's stack.
	let Some(state) = (unsafe { opaque.cast::<StatsPrintState>().as_mut() }) else {
		return;
	};

	if state.panic.is_some() || msg.is_null() || state.buf.len() >= state.max_len {
		return;
	}

	// SAFETY: jemalloc guarantees `msg` is a valid NUL-terminated C string.
	let msg = unsafe { CStr::from_ptr(msg) };

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

macro_rules! mallctl {
	($name:literal) => {{
		static KEY: OnceLock<Key> = OnceLock::new();

		KEY.get_or_init(|| key($name).expect("failed to translate name into mib key"))
	}};
}

pub mod this_thread {
	use super::{Debug, Key, OnceLock, Result, c_uint, is_nonzero, key, math};

	pub fn trim() -> Result { decay().and_then(|()| purge()) }

	pub fn purge() -> Result {
		// SAFETY: `arena.<i>.purge` is an `arena.<i>.*` notification ctl.
		unsafe { notify(mallctl!("arena.0.purge")) }
	}

	pub fn decay() -> Result {
		// SAFETY: `arena.<i>.decay` is an `arena.<i>.*` notification ctl.
		unsafe { notify(mallctl!("arena.0.decay")) }
	}

	pub fn idle() -> Result {
		// SAFETY: `thread.idle` is a non-indexed notification ctl.
		unsafe { super::notify(mallctl!("thread.idle")) }
	}

	pub fn flush() -> Result {
		// SAFETY: `thread.tcache.flush` is a non-indexed notification ctl.
		unsafe { super::notify(mallctl!("thread.tcache.flush")) }
	}

	pub fn set_muzzy_decay(decay_ms: isize) -> Result<isize> {
		// SAFETY: `arena.<i>.muzzy_decay_ms` has C type `ssize_t`.
		unsafe { set(mallctl!("arena.0.muzzy_decay_ms"), decay_ms) }
	}

	pub fn get_muzzy_decay() -> Result<isize> {
		// SAFETY: `arena.<i>.muzzy_decay_ms` has C type `ssize_t`.
		unsafe { get(mallctl!("arena.0.muzzy_decay_ms")) }
	}

	pub fn set_dirty_decay(decay_ms: isize) -> Result<isize> {
		// SAFETY: `arena.<i>.dirty_decay_ms` has C type `ssize_t`.
		unsafe { set(mallctl!("arena.0.dirty_decay_ms"), decay_ms) }
	}

	pub fn get_dirty_decay() -> Result<isize> {
		// SAFETY: `arena.<i>.dirty_decay_ms` has C type `ssize_t`.
		unsafe { get(mallctl!("arena.0.dirty_decay_ms")) }
	}

	pub fn cache_enable(enable: bool) -> Result<bool> {
		// SAFETY: `thread.tcache.enabled` has C type `bool` (1 byte on non-MSVC).
		unsafe { super::set::<u8>(mallctl!("thread.tcache.enabled"), enable.into()) }
			.map(is_nonzero!())
	}

	pub fn is_cache_enabled() -> Result<bool> {
		// SAFETY: `thread.tcache.enabled` has C type `bool` (1 byte on non-MSVC).
		unsafe { super::get::<u8>(mallctl!("thread.tcache.enabled")) }.map(is_nonzero!())
	}

	pub fn set_arena(id: usize) -> Result<usize> {
		// SAFETY: `thread.arena` has C type `unsigned`.
		unsafe { super::set::<c_uint>(mallctl!("thread.arena"), id.try_into()?) }
			.and_then(math::try_into)
	}

	pub fn arena_id() -> Result<usize> {
		// SAFETY: `thread.arena` has C type `unsigned`.
		unsafe { super::get::<c_uint>(mallctl!("thread.arena")) }.and_then(math::try_into)
	}

	pub fn prof_enable(enable: bool) -> Result<bool> {
		// SAFETY: `thread.prof.active` has C type `bool` (1 byte on non-MSVC).
		unsafe { super::set::<u8>(mallctl!("thread.prof.active"), enable.into()) }
			.map(is_nonzero!())
	}

	pub fn is_prof_enabled() -> Result<bool> {
		// SAFETY: `thread.prof.active` has C type `bool` (1 byte on non-MSVC).
		unsafe { super::get::<u8>(mallctl!("thread.prof.active")) }.map(is_nonzero!())
	}

	pub fn reset_peak() -> Result {
		// SAFETY: `thread.peak.reset` is a non-indexed notification ctl.
		unsafe { super::notify(mallctl!("thread.peak.reset")) }
	}

	pub fn peak() -> Result<u64> {
		// SAFETY: `thread.peak.read` has C type `uint64_t`.
		unsafe { super::get(mallctl!("thread.peak.read")) }
	}

	#[inline]
	#[must_use]
	pub fn allocated() -> u64 {
		// SAFETY: `thread.allocated` has C type `uint64_t`.
		unsafe { super::get(mallctl!("thread.allocated")) }.unwrap_or_default()
	}

	#[inline]
	#[must_use]
	pub fn deallocated() -> u64 {
		// SAFETY: `thread.deallocated` has C type `uint64_t`.
		unsafe { super::get(mallctl!("thread.deallocated")) }.unwrap_or_default()
	}

	/// # Safety
	/// Caller must ensure `key` is an `arena.<i>.*` notification MIB
	/// (`key.len() >= 2`).
	// `arena_id()` returns this thread's own valid arena (< narenas < ARENA_LIMIT),
	// so we skip `validate_arena_id()` to avoid locking `ctl_mtx` on hot paths like
	// `decay()`.
	unsafe fn notify(key: &Key) -> Result {
		let mut key = key.clone();
		key[1] = arena_id()?;
		// SAFETY: Caller guarantees `key` is an `arena.<i>.*` notification MIB
		// and `arena_id()` returns a valid arena index.
		unsafe { super::notify(&key) }
	}

	/// # Safety
	/// Caller must ensure `key` is an `arena.<i>.*` MIB and `T` matches its
	/// exact C type.
	unsafe fn set<T>(key: &Key, val: T) -> Result<T>
	where
		T: Copy + Debug,
	{
		let mut key = key.clone();
		// `arena_id()` is already a valid arena index; skip `validate_arena_id()`.
		key[1] = arena_id()?;
		// SAFETY: Caller guarantees `T` matches `key`.
		unsafe { super::set(&key, val) }
	}

	/// # Safety
	/// Caller must ensure `key` is an `arena.<i>.*` MIB and `T` matches its
	/// exact C type.
	unsafe fn get<T>(key: &Key) -> Result<T>
	where
		T: Copy + Debug,
	{
		let mut key = key.clone();
		// `arena_id()` is already a valid arena index; skip `validate_arena_id()`.
		key[1] = arena_id()?;
		// SAFETY: Caller guarantees `T` matches `key`.
		unsafe { super::get(&key) }
	}
}

pub fn stats_reset() -> Result {
	// SAFETY: `stats.mutexes.reset` is a non-indexed notification ctl.
	unsafe { notify(mallctl!("stats.mutexes.reset")) }
}

pub fn prof_reset() -> Result {
	// SAFETY: `prof.reset` is a non-indexed notification ctl.
	unsafe { notify(mallctl!("prof.reset")) }
}

pub fn prof_enable(enable: bool) -> Result<bool> {
	// SAFETY: `prof.active` has C type `bool` (1 byte on non-MSVC).
	unsafe { set::<u8>(mallctl!("prof.active"), enable.into()) }.map(is_nonzero!())
}

pub fn is_prof_enabled() -> Result<bool> {
	// SAFETY: `prof.active` has C type `bool` (1 byte on non-MSVC).
	unsafe { get::<u8>(mallctl!("prof.active")) }.map(is_nonzero!())
}

pub fn trim<I: Into<Option<usize>> + Copy>(arena: I) -> Result {
	decay(arena).and_then(|()| purge(arena))
}

pub fn purge<I: Into<Option<usize>>>(arena: I) -> Result {
	// SAFETY: `arena.<i>.purge` is an `arena.<i>.*` notification ctl.
	unsafe { notify_by_arena(arena.into(), mallctl!("arena.4096.purge")) }
}

pub fn decay<I: Into<Option<usize>>>(arena: I) -> Result {
	// SAFETY: `arena.<i>.decay` is an `arena.<i>.*` notification ctl.
	unsafe { notify_by_arena(arena.into(), mallctl!("arena.4096.decay")) }
}

pub fn set_muzzy_decay<I: Into<Option<usize>>>(arena: I, decay_ms: isize) -> Result<isize> {
	match arena.into() {
		// SAFETY: `arena.<i>.muzzy_decay_ms` has C type `ssize_t`.
		| Some(arena) => unsafe {
			set_by_arena(arena, mallctl!("arena.0.muzzy_decay_ms"), decay_ms)
		},
		// SAFETY: `arenas.muzzy_decay_ms` has C type `ssize_t`.
		| _ => unsafe { set(mallctl!("arenas.muzzy_decay_ms"), decay_ms) },
	}
}

pub fn set_dirty_decay<I: Into<Option<usize>>>(arena: I, decay_ms: isize) -> Result<isize> {
	match arena.into() {
		// SAFETY: `arena.<i>.dirty_decay_ms` has C type `ssize_t`.
		| Some(arena) => unsafe {
			set_by_arena(arena, mallctl!("arena.0.dirty_decay_ms"), decay_ms)
		},
		// SAFETY: `arenas.dirty_decay_ms` has C type `ssize_t`.
		| _ => unsafe { set(mallctl!("arenas.dirty_decay_ms"), decay_ms) },
	}
}

pub fn background_thread_enable(enable: bool) -> Result<bool> {
	// SAFETY: `background_thread` has C type `bool` (1 byte on non-MSVC).
	unsafe { set::<u8>(mallctl!("background_thread"), enable.into()) }.map(is_nonzero!())
}

#[inline]
#[must_use]
pub fn is_affine_arena() -> bool { is_percpu_arena() || is_phycpu_arena() }

#[inline]
#[must_use]
pub fn is_percpu_arena() -> bool { percpu_arenas().is_ok_and(is_equal_to!("percpu")) }

#[inline]
#[must_use]
pub fn is_phycpu_arena() -> bool { percpu_arenas().is_ok_and(is_equal_to!("phycpu")) }

pub fn percpu_arenas() -> Result<&'static str> {
	// SAFETY: `opt.percpu_arena` has C type `const char *`.
	let ptr = unsafe { get::<*const c_char>(mallctl!("opt.percpu_arena")) }?;
	if ptr.is_null() {
		return Err(err!("mallctl: opt.percpu_arena returned NULL"));
	}

	// SAFETY: `ptr` is non-null and points to a static NUL-terminated string
	// returned by jemalloc for `opt.percpu_arena`.
	let cstr = unsafe { CStr::from_ptr(ptr) };
	cstr.to_str().map_err(Into::into)
}

pub fn arenas() -> Result<usize> {
	// SAFETY: `arenas.narenas` has C type `unsigned`.
	unsafe { get::<c_uint>(mallctl!("arenas.narenas")) }.and_then(math::try_into)
}

pub fn inc_epoch() -> Result<u64> {
	// SAFETY: `epoch` has C type `uint64_t`.
	unsafe { xchg(mallctl!("epoch"), 1_u64) }
}

pub fn acq_epoch() -> Result<u64> {
	// SAFETY: `epoch` has C type `uint64_t`.
	unsafe { get(mallctl!("epoch")) }
}

fn validate_arena_id(id: usize, allow_all: bool) -> Result<usize> {
	if allow_all && id == ARENAS_ALL {
		return Ok(id);
	}

	let narenas = arenas()?.min(ARENA_LIMIT);
	if id < narenas {
		Ok(id)
	} else {
		Err(err!("mallctl: invalid arena index {id} (narenas: {narenas})"))
	}
}

/// # Safety
/// Caller must ensure `key` is an `arena.<i>.*` notification MIB
/// (`key.len() >= 2`).
unsafe fn notify_by_arena(id: Option<usize>, key: &Key) -> Result {
	let mut key = key.clone();
	key[1] = match id {
		| Some(id) => validate_arena_id(id, true)?,
		| None => ARENAS_ALL,
	};
	// SAFETY: Caller guarantees `key` is an `arena.<i>.*` notification MIB and
	// `key[1]` has been validated above.
	unsafe { notify(&key) }
}

/// # Safety
/// Caller must ensure `key` is an `arena.<i>.*` MIB (`key.len() >= 2`) and `T`
/// matches its exact C type.
unsafe fn set_by_arena<T>(id: usize, key: &Key, val: T) -> Result<T>
where
	T: Copy + Debug,
{
	let mut key = key.clone();
	key[1] = validate_arena_id(id, false)?;
	// SAFETY: Caller guarantees `T` matches `key`.
	unsafe { set(&key, val) }
}

/// # Safety
/// Caller must ensure `key` is a valid notification MIB and any indexed arena
/// component is within bounds.
unsafe fn notify(key: &Key) -> Result {
	// SAFETY: jemalloc notification ctls require `oldp`, `oldlenp`, and `newp`
	// to be NULL and `newlen` to be 0 (otherwise `EPERM` is returned).
	let ret = unsafe {
		ffi::mallctlbymib(
			key.as_ptr(),
			key.len(),
			ptr::null_mut(),
			ptr::null_mut(),
			ptr::null_mut(),
			0,
		)
	};
	if ret == 0 {
		Ok(())
	} else {
		Err(err!("mallctl: {}", std::io::Error::from_raw_os_error(ret)))
	}
}

/// # Safety
/// Caller must ensure `T` matches the exact C type expected by `key`.
// Does not advance `epoch`; callers reading cached `stats.*` must call
// `inc_epoch()` explicitly.
unsafe fn set<T>(key: &Key, val: T) -> Result<T>
where
	T: Copy + Debug,
{
	// SAFETY: Caller guarantees `T` matches `key`.
	unsafe { xchg(key, val) }
}

/// # Safety
/// Caller must ensure `T` matches the exact C type expected by `key`.
#[tracing::instrument(
	name = "get",
	level = "trace",
	skip_all,
	fields(?key)
)]
unsafe fn get<T>(key: &Key) -> Result<T>
where
	T: Copy + Debug,
{
	// SAFETY: Caller guarantees `T` matches the C type returned for `key`.
	unsafe { mallctl::raw::read_mib(key.as_slice()) }.map_err(map_err)
}

/// # Safety
/// Caller must ensure `T` matches the exact C type expected by `key`.
#[tracing::instrument(
	name = "xchg",
	level = "trace",
	skip_all,
	fields(?key, ?val)
)]
unsafe fn xchg<T>(key: &Key, val: T) -> Result<T>
where
	T: Copy + Debug,
{
	// SAFETY: Caller guarantees `T` matches the C type expected by `key`.
	unsafe { mallctl::raw::update_mib(key.as_slice(), val) }.map_err(map_err)
}

fn key(name: &str) -> Result<Key> {
	// tikv asserts the output buffer length is tight to the number of required
	// mibs so we slice that down here.
	let segs = name.chars().filter(is_equal_to!(&'.')).count().try_add(1)?;
	if segs > KEY_SEGS {
		return Err(err!("mallctl: key has too many segments ({segs} > {KEY_SEGS})"));
	}

	let name = self::name(name)?;
	let mut buf = [0_usize; KEY_SEGS];
	mallctl::raw::name_to_mib(name.as_slice(), &mut buf[0..segs])
		.map_err(map_err)
		.map(move |()| buf.into_iter().take(segs).collect())
}

fn name(name: &str) -> Result<Name> {
	if name.as_bytes().contains(&0) {
		return Err(err!("mallctl: key contains interior NUL byte"));
	}

	let mut buf = Name::new();
	buf.try_extend_from_slice(name.as_bytes())?;
	buf.try_extend_from_slice(b"\0")?;

	Ok(buf)
}

fn map_err(error: tikv_jemalloc_ctl::Error) -> crate::Error {
	err!("mallctl: {}", error.to_string())
}
