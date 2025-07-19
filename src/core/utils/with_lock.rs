//! Traits for explicitly scoping the lifetime of locks.

use std::{
	future::Future,
	sync::{Arc, Mutex},
};

pub trait WithLock<T: ?Sized> {
	/// Acquires a lock and executes the given closure with the locked data,
	/// returning the result.
	fn with_lock<R, F>(&self, f: F) -> R
	where
		F: FnMut(&mut T) -> R;
}

impl<T> WithLock<T> for Mutex<T> {
	fn with_lock<R, F>(&self, mut f: F) -> R
	where
		F: FnMut(&mut T) -> R,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().unwrap();
		f(&mut data_guard)
		// Lock is released here when `data_guard` goes out of scope.
	}
}

impl<T> WithLock<T> for Arc<Mutex<T>> {
	fn with_lock<R, F>(&self, mut f: F) -> R
	where
		F: FnMut(&mut T) -> R,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().unwrap();
		f(&mut data_guard)
		// Lock is released here when `data_guard` goes out of scope.
	}
}

impl<R: lock_api::RawMutex, T: ?Sized> WithLock<T> for lock_api::Mutex<R, T> {
	fn with_lock<Ret, F>(&self, mut f: F) -> Ret
	where
		F: FnMut(&mut T) -> Ret,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock();
		f(&mut data_guard)
		// Lock is released here when `data_guard` goes out of scope.
	}
}

impl<R: lock_api::RawMutex, T: ?Sized> WithLock<T> for Arc<lock_api::Mutex<R, T>> {
	fn with_lock<Ret, F>(&self, mut f: F) -> Ret
	where
		F: FnMut(&mut T) -> Ret,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock();
		f(&mut data_guard)
		// Lock is released here when `data_guard` goes out of scope.
	}
}

pub trait WithLockAsync<T> {
	/// Acquires a lock and executes the given closure with the locked data,
	/// returning the result.
	fn with_lock<R, F>(&self, f: F) -> impl Future<Output = R>
	where
		F: FnMut(&mut T) -> R;

	/// Acquires a lock and executes the given async closure with the locked
	/// data.
	fn with_lock_async<R, F>(&self, f: F) -> impl std::future::Future<Output = R>
	where
		F: AsyncFnMut(&mut T) -> R;
}

impl<T> WithLockAsync<T> for futures::lock::Mutex<T> {
	async fn with_lock<R, F>(&self, mut f: F) -> R
	where
		F: FnMut(&mut T) -> R,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().await;
		f(&mut data_guard)
		// Lock is released here when `data_guard` goes out of scope.
	}

	async fn with_lock_async<R, F>(&self, mut f: F) -> R
	where
		F: AsyncFnMut(&mut T) -> R,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().await;
		f(&mut data_guard).await
		// Lock is released here when `data_guard` goes out of scope.
	}
}

impl<T> WithLockAsync<T> for Arc<futures::lock::Mutex<T>> {
	async fn with_lock<R, F>(&self, mut f: F) -> R
	where
		F: FnMut(&mut T) -> R,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().await;
		f(&mut data_guard)
		// Lock is released here when `data_guard` goes out of scope.
	}

	async fn with_lock_async<R, F>(&self, mut f: F) -> R
	where
		F: AsyncFnMut(&mut T) -> R,
	{
		// The locking and unlocking logic is hidden inside this function.
		let mut data_guard = self.lock().await;
		f(&mut data_guard).await
		// Lock is released here when `data_guard` goes out of scope.
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_with_lock_return_value() {
		let mutex = Mutex::new(5);
		let result = mutex.with_lock(|v| {
			*v += 1;
			*v * 2
		});
		assert_eq!(result, 12);
		let value = mutex.lock().unwrap();
		assert_eq!(*value, 6);
	}

	#[test]
	fn test_with_lock_unit_return() {
		let mutex = Mutex::new(10);
		mutex.with_lock(|v| {
			*v += 2;
		});
		let value = mutex.lock().unwrap();
		assert_eq!(*value, 12);
	}

	#[test]
	fn test_with_lock_arc_mutex() {
		let mutex = Arc::new(Mutex::new(1));
		let result = mutex.with_lock(|v| {
			*v *= 10;
			*v
		});
		assert_eq!(result, 10);
		assert_eq!(*mutex.lock().unwrap(), 10);
	}

	#[tokio::test]
	async fn test_with_lock_async_return_value() {
		use futures::lock::Mutex as AsyncMutex;
		let mutex = AsyncMutex::new(7);
		let result = mutex
			.with_lock(|v| {
				*v += 3;
				*v * 2
			})
			.await;
		assert_eq!(result, 20);
		let value = mutex.lock().await;
		assert_eq!(*value, 10);
	}

	#[tokio::test]
	async fn test_with_lock_async_unit_return() {
		use futures::lock::Mutex as AsyncMutex;
		let mutex = AsyncMutex::new(100);
		mutex
			.with_lock(|v| {
				*v -= 50;
			})
			.await;
		let value = mutex.lock().await;
		assert_eq!(*value, 50);
	}

	#[tokio::test]
	async fn test_with_lock_async_closure() {
		use futures::lock::Mutex as AsyncMutex;
		let mutex = AsyncMutex::new(1);
		mutex
			.with_lock_async(async |v| {
				*v += 9;
			})
			.await;
		let value = mutex.lock().await;
		assert_eq!(*value, 10);
	}

	#[tokio::test]
	async fn test_with_lock_async_arc_mutex() {
		use futures::lock::Mutex as AsyncMutex;
		let mutex = Arc::new(AsyncMutex::new(2));
		mutex
			.with_lock_async(async |v: &mut i32| {
				*v *= 5;
			})
			.await;
		let value = mutex.lock().await;
		assert_eq!(*value, 10);
	}
}
