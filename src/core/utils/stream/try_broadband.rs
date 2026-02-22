//! Synchronous combinator extensions to futures::TryStream

use std::result::Result;

use futures::{TryFuture, TryStream, TryStreamExt};

use super::automatic_width;

/// Concurrency extensions to augment futures::TryStreamExt. broad_ combinators
/// produce out-of-order
pub trait TryBroadbandExt<T, E>
where
	Self: TryStream<Ok = T, Error = E, Item = Result<T, E>> + Send + Sized,
{
	fn broadn_and_then<U, F, Fut, N>(
		self,
		n: N,
		f: F,
	) -> impl TryStream<Ok = U, Error = E, Item = Result<U, E>> + Send
	where
		N: Into<Option<usize>>,
		F: Fn(Self::Ok) -> Fut + Send,
		Fut: TryFuture<Ok = U, Error = E, Output = Result<U, E>> + Send;

	fn broad_and_then<U, F, Fut>(
		self,
		f: F,
	) -> impl TryStream<Ok = U, Error = E, Item = Result<U, E>> + Send
	where
		F: Fn(Self::Ok) -> Fut + Send,
		Fut: TryFuture<Ok = U, Error = E, Output = Result<U, E>> + Send,
	{
		self.broadn_and_then(None, f)
	}
}

impl<T, E, S> TryBroadbandExt<T, E> for S
where
	S: TryStream<Ok = T, Error = E, Item = Result<T, E>> + Send + Sized,
{
	fn broadn_and_then<U, F, Fut, N>(
		self,
		n: N,
		f: F,
	) -> impl TryStream<Ok = U, Error = E, Item = Result<U, E>> + Send
	where
		N: Into<Option<usize>>,
		F: Fn(Self::Ok) -> Fut + Send,
		Fut: TryFuture<Ok = U, Error = E, Output = Result<U, E>> + Send,
	{
		self.map_ok(f)
			.try_buffer_unordered(n.into().unwrap_or_else(automatic_width))
	}
}
