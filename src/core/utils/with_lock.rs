//! Traits for explicitly scoping the lifetime of locks.

use std::sync::{Arc, Mutex};

pub trait WithLock<T> {
	/// Acquires a lock and executes the given closure with the locked data.
	fn with_lock<F>(&self, f: F)
	where
		F: FnMut(&mut T);
}

impl<T> WithLock<T> for Mutex<T> {
	fn with_lock<F>(&self, mut f: F)
	where
		F: FnMut(&mut T),
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().unwrap();
		f(&mut data_guard);
		// Lock is released here when `data_guard` goes out of scope.
	}
}

impl<T> WithLock<T> for Arc<Mutex<T>> {
	fn with_lock<F>(&self, mut f: F)
	where
		F: FnMut(&mut T),
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().unwrap();
		f(&mut data_guard);
		// Lock is released here when `data_guard` goes out of scope.
	}
}

pub trait WithLockAsync<T> {
	/// Acquires a lock and executes the given closure with the locked data.
	fn with_lock<F>(&self, f: F) -> impl Future<Output = ()>
	where
		F: FnMut(&mut T);
}

impl<T> WithLockAsync<T> for futures::lock::Mutex<T> {
	async fn with_lock<F>(&self, mut f: F)
	where
		F: FnMut(&mut T),
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().await;
		f(&mut data_guard);
		// Lock is released here when `data_guard` goes out of scope.
	}
}

impl<T> WithLockAsync<T> for Arc<futures::lock::Mutex<T>> {
	async fn with_lock<F>(&self, mut f: F)
	where
		F: FnMut(&mut T),
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().await;
		f(&mut data_guard);
		// Lock is released here when `data_guard` goes out of scope.
	}
}
