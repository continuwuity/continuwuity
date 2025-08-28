use std::{convert::AsRef, fmt::Debug, sync::Arc};

use conduwuit::{Result, implement};
use futures::{Stream, StreamExt, TryStreamExt, future};
use serde::{Deserialize, Serialize};

use crate::keyval::{KeyVal, result_deserialize, serialize_key};

/// Iterate key-value entries in the map where the key matches a prefix.
///
/// - Query is serialized
/// - Result is deserialized
#[implement(super::Map)]
pub fn stream_prefix<'a, K, V, P>(
	self: &'a Arc<Self>,
	prefix: &P,
) -> impl Stream<Item = Result<KeyVal<'a, K, V>>> + Send + use<'a, K, V, P>
where
	P: Serialize + ?Sized + Debug,
	K: Deserialize<'a> + Send,
	V: Deserialize<'a> + Send,
{
	self.stream_prefix_raw(prefix)
		.map(result_deserialize::<K, V>)
}

/// Iterate key-value entries in the map where the key matches a prefix.
///
/// - Query is serialized
/// - Result is raw
#[implement(super::Map)]
#[tracing::instrument(skip(self), level = "trace")]
pub fn stream_prefix_raw<P>(
	self: &Arc<Self>,
	prefix: &P,
) -> impl Stream<Item = Result<KeyVal<'_>>> + Send + use<'_, P>
where
	P: Serialize + ?Sized + Debug,
{
	let key = serialize_key(prefix).expect("failed to serialize query key");
	self.raw_stream_from(&key)
		.try_take_while(move |(k, _): &KeyVal<'_>| future::ok(k.starts_with(&key)))
}

/// Iterate key-value entries in the map where the key matches a prefix.
///
/// - Query is raw
/// - Result is deserialized
#[implement(super::Map)]
pub fn stream_raw_prefix<'a, K, V, P>(
	self: &'a Arc<Self>,
	prefix: &'a P,
) -> impl Stream<Item = Result<KeyVal<'a, K, V>>> + Send + 'a
where
	P: AsRef<[u8]> + ?Sized + Debug + Sync + 'a,
	K: Deserialize<'a> + Send + 'a,
	V: Deserialize<'a> + Send + 'a,
{
	self.raw_stream_prefix(prefix)
		.map(result_deserialize::<K, V>)
}

/// Iterate key-value entries in the map where the key matches a prefix.
///
/// - Query is raw
/// - Result is raw
#[implement(super::Map)]
pub fn raw_stream_prefix<'a, P>(
	self: &'a Arc<Self>,
	prefix: &'a P,
) -> impl Stream<Item = Result<KeyVal<'a>>> + Send + 'a
where
	P: AsRef<[u8]> + ?Sized + Debug + Sync + 'a,
{
	self.raw_stream_from(prefix)
		.try_take_while(|(k, _): &KeyVal<'_>| future::ok(k.starts_with(prefix.as_ref())))
}
