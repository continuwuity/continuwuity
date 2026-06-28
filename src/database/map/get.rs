use std::{convert::AsRef, fmt::Debug, sync::Arc};

use conduwuit::{Err, Result, err, utils::result::MapExpect};
use futures::{Future, FutureExt, TryFutureExt, future::ready};
use rocksdb::{DBPinnableSlice, ReadOptions};
use tokio::task;

use crate::{
	Handle,
	util::{is_incomplete, map_err, or_else},
};

impl super::Map {
	/// Fetch a value from the database into cache, returning a reference-handle
	/// asynchronously. The key is referenced directly to perform the query.
	#[tracing::instrument(skip(self, key), fields(%self), level = "trace")]
	pub fn get<K>(
		self: &Arc<Self>,
		key: &K,
	) -> impl Future<Output = Result<Handle<'_>>> + Send + use<'_, K>
	where
		K: AsRef<[u8]> + Debug + ?Sized,
	{
		use crate::pool::Get;

		let cached = self.get_cached(key);
		if matches!(cached, Err(_) | Ok(Some(_))) {
			return task::consume_budget()
				.map(move |()| cached.map_expect("data found in cache"))
				.boxed();
		}

		debug_assert!(matches!(cached, Ok(None)), "expected status Incomplete");
		let cmd = Get {
			map: self.clone(),
			key: [key.as_ref().into()].into(),
			res: None,
		};

		self.db
			.pool
			.execute_get(cmd)
			.and_then(|mut res| ready(res.remove(0)))
			.boxed()
	}

	/// Fetch a value from the cache without I/O.
	#[tracing::instrument(skip(self, key), name = "cache", level = "trace")]
	pub(crate) fn get_cached<K>(&self, key: &K) -> Result<Option<Handle<'_>>>
	where
		K: AsRef<[u8]> + Debug + ?Sized,
	{
		let res = self.get_blocking_opts(key, &self.cache_read_options);
		cached_handle_from(res)
	}

	/// Fetch a value from the database into cache, returning a
	/// reference-handle. The key is referenced directly to perform the query.
	/// This is a thread- blocking call.
	#[tracing::instrument(skip(self, key), name = "blocking", level = "trace")]
	pub fn get_blocking<K>(&self, key: &K) -> Result<Handle<'_>>
	where
		K: AsRef<[u8]> + ?Sized,
	{
		let res = self.get_blocking_opts(key, &self.read_options);
		handle_from(res)
	}

	fn get_blocking_opts<K>(
		&self,
		key: &K,
		read_options: &ReadOptions,
	) -> Result<Option<DBPinnableSlice<'_>>, rocksdb::Error>
	where
		K: AsRef<[u8]> + ?Sized,
	{
		self.db.db.get_pinned_cf_opt(&self.cf(), key, read_options)
	}
}

#[inline]
pub(super) fn handle_from(
	result: Result<Option<DBPinnableSlice<'_>>, rocksdb::Error>,
) -> Result<Handle<'_>> {
	result
		.map_err(map_err)?
		.map(Handle::from)
		.ok_or(err!(Request(NotFound("Not found in database"))))
}

#[inline]
pub(super) fn cached_handle_from(
	result: Result<Option<DBPinnableSlice<'_>>, rocksdb::Error>,
) -> Result<Option<Handle<'_>>> {
	match result {
		// cache hit; not found
		| Ok(None) => Err!(Request(NotFound("Not found in database"))),

		// cache hit; value found
		| Ok(Some(result)) => Ok(Some(Handle::from(result))),

		// cache miss; unknown
		| Err(error) if is_incomplete(&error) => Ok(None),

		// some other error occurred
		| Err(error) => or_else(error),
	}
}
