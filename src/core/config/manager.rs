use std::{
	ops::Deref,
	ptr::NonNull,
	sync::{
		Arc,
		atomic::{AtomicPtr, Ordering},
	},
};

use super::Config;
use crate::{Result, SyncMutex};

/// The configuration manager is an indirection to reload the configuration for
/// the server while it is running. In order to not burden or clutter the many
/// callsites which query for configuration items, this object implements Deref
/// for the actively loaded configuration.
///
/// Every installed `Arc<Config>` is kept at a stable heap address in `retired`
/// for the entire lifetime of `Manager` (intentionally retaining prior configs
/// across the rare, admin-triggered reloads until shutdown). Because
/// `Deref::deref(&self)` ties the returned `&Arc<Config>` lifetime to `&self`,
/// readers can perform a single `Acquire` load with no locking, no TLS, and no
/// risk of dangling or aliasing UB across reloads.
pub struct Manager {
	active: AtomicPtr<Arc<Config>>,
	/// Append-only owner of every `Box<Arc<Config>>` ever published to
	/// `active` (including the currently active config). Entries are only
	/// freed when `Manager` itself is dropped (`&mut self`), guaranteeing that
	/// any `&Arc<Config>` returned by `Deref::deref` remains valid and
	/// immutable for the lifetime of `self`.
	retired: SyncMutex<Vec<PinnedConfig>>,
}

/// Heap-pinned `Arc<Config>` stored as a raw `NonNull` rather than `Box` so
/// moving it (e.g., when `Vec` reallocates) never retags or invalidates raw
/// pointers previously published to `Manager::active`.
struct PinnedConfig(NonNull<Arc<Config>>);

// SAFETY: `Arc<Config>` is `Send + Sync`, and `PinnedConfig` uniquely owns the
// `Box<Arc<Config>>` allocation until it is dropped in `Manager::drop`.
unsafe impl Send for PinnedConfig {}

impl PinnedConfig {
	fn new(config: Config) -> Self {
		let ptr = Box::into_raw(Box::new(Arc::new(config)));
		Self(NonNull::new(ptr).expect("Box::into_raw returned null"))
	}

	#[inline]
	const fn as_ptr(&self) -> *mut Arc<Config> { self.0.as_ptr() }
}

impl Drop for PinnedConfig {
	fn drop(&mut self) {
		// SAFETY: `self.0` was created via `Box::into_raw` in
		// `PinnedConfig::new` and is only dropped when `Manager` itself is
		// dropped (`&mut self`), after all borrows derived from `Deref::deref`
		// have ended.
		unsafe {
			drop(Box::from_raw(self.0.as_ptr()));
		}
	}
}

impl Manager {
	pub(crate) fn new(config: Config) -> Self {
		let pinned = PinnedConfig::new(config);
		let ptr = pinned.as_ptr();
		Self {
			active: AtomicPtr::new(ptr),
			retired: SyncMutex::new(vec![pinned]),
		}
	}

	/// Update the active configuration, returning prior configuration.
	#[tracing::instrument(skip_all, level = "info")]
	pub fn update(&self, config: Config) -> Result<Arc<Config>> {
		let pinned = PinnedConfig::new(config);
		let new_ptr = pinned.as_ptr();

		let mut retired = self.retired.lock();
		// Push before publishing to `active` so ownership is recorded even if
		// `Vec::push` were to unwind.
		retired.push(pinned);

		// `Release` publishes the initialized `Box<Arc<Config>>` to readers
		// performing `Acquire` loads in `Deref::deref`.
		let old_ptr = self.active.swap(new_ptr, Ordering::AcqRel);

		// SAFETY: `old_ptr` points to a `PinnedConfig` still owned by `retired`
		// and cannot be freed until `Manager` is dropped.
		let old = unsafe { Arc::clone(&*old_ptr) };

		Ok(old)
	}
}

impl Deref for Manager {
	type Target = Arc<Config>;

	#[inline]
	fn deref(&self) -> &Self::Target {
		let ptr = self.active.load(Ordering::Acquire);

		// SAFETY: `ptr` was initialized in `Manager::new` or published via
		// `Ordering::AcqRel` in `Manager::update` and points to a
		// `PinnedConfig` in `self.retired`. Entries in `self.retired` are never
		// removed or mutated until `Manager` is dropped (`&mut self`), so the
		// returned `&'self Arc<Config>` remains valid and immutable for the
		// entire borrow of `self`.
		unsafe { &*ptr }
	}
}

#[cfg(test)]
mod tests {
	use std::thread;

	use super::*;

	fn test_config(server_name: &str) -> Config {
		use figment::{
			Figment,
			providers::{Format, Toml},
		};

		let raw = format!("server_name = \"{server_name}\"\ndatabase_path = \"/tmp\"\n");
		Figment::new()
			.merge(Toml::string(&raw))
			.extract()
			.expect("valid test config")
	}

	#[test]
	fn deref_survives_many_reloads() {
		let manager = Manager::new(test_config("initial.example"));
		let first: &Arc<Config> = &manager;
		let first_name: &str = first.server_name.as_str();

		for i in 0..32 {
			let prev = manager
				.update(test_config(&format!("reload-{i}.example")))
				.expect("update succeeds");
			if i == 0 {
				assert_eq!(prev.server_name.as_str(), "initial.example");
			}
		}

		// Original reference remains valid and unchanged after > HISTORY
		// reloads.
		assert_eq!(first_name, "initial.example");
		assert_eq!(manager.server_name.as_str(), "reload-31.example");
	}

	#[test]
	fn concurrent_reads_and_updates() {
		let manager = Manager::new(test_config("start.example"));

		thread::scope(|s| {
			for _ in 0..4 {
				s.spawn(|| {
					for _ in 0..200 {
						let cfg: &Arc<Config> = &manager;
						assert!(cfg.server_name.as_str().ends_with(".example"));
					}
				});
			}

			s.spawn(|| {
				for i in 0..32 {
					let _ = manager
						.update(test_config(&format!("iter-{i}.example")))
						.expect("update succeeds");
				}
			});
		});
	}

	#[test]
	fn reference_outlives_scoped_thread() {
		let manager = Manager::new(test_config("scoped.example"));
		let cfg_ref: &Arc<Config> = thread::scope(|s| s.spawn(|| &*manager).join().unwrap());
		assert_eq!(cfg_ref.server_name.as_str(), "scoped.example");
	}
}
