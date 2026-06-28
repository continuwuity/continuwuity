use std::{fmt::Debug, future::Future, sync::Arc};

use futures::stream::StreamExt;
use serde::Serialize;

impl super::Map {
	/// Count the total number of entries in the map.
	#[inline]
	pub fn count(self: &Arc<Self>) -> impl Future<Output = usize> + Send + '_ {
		self.raw_keys().count()
	}

	/// Count the number of entries in the map starting from a lower-bound.
	///
	/// - From is a structured key
	#[inline]
	pub fn count_from<'a, P>(
		self: &'a Arc<Self>,
		from: &P,
	) -> impl Future<Output = usize> + Send + 'a + use<'a, P>
	where
		P: Serialize + ?Sized + Debug + 'a,
	{
		self.keys_from_raw(from).count()
	}

	/// Count the number of entries in the map starting from a lower-bound.
	///
	/// - From is a raw
	#[inline]
	pub fn raw_count_from<'a, P>(
		self: &'a Arc<Self>,
		from: &'a P,
	) -> impl Future<Output = usize> + Send + 'a
	where
		P: AsRef<[u8]> + ?Sized + Debug + Sync + 'a,
	{
		self.raw_keys_from(from).count()
	}

	/// Count the number of entries in the map matching a prefix.
	///
	/// - Prefix is structured key
	#[inline]
	pub fn count_prefix<'a, P>(
		self: &'a Arc<Self>,
		prefix: &P,
	) -> impl Future<Output = usize> + Send + 'a + use<'a, P>
	where
		P: Serialize + ?Sized + Debug + 'a,
	{
		self.keys_prefix_raw(prefix).count()
	}

	/// Count the number of entries in the map matching a prefix.
	///
	/// - Prefix is raw
	#[inline]
	pub fn raw_count_prefix<'a, P>(
		self: &'a Arc<Self>,
		prefix: &'a P,
	) -> impl Future<Output = usize> + Send + 'a
	where
		P: AsRef<[u8]> + ?Sized + Debug + Sync + 'a,
	{
		self.raw_keys_prefix(prefix).count()
	}
}
