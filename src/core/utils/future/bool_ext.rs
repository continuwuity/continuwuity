//! Extended external extensions to futures::FutureExt

use futures::{
	Future, FutureExt,
	future::{try_join, try_join_all},
};

pub trait BoolExt
where
	Self: Future<Output = bool> + Send,
{
	fn and<B>(self, b: B) -> impl Future<Output = bool> + Send
	where
		B: Future<Output = bool> + Send,
		Self: Sized;

	fn or<B>(self, b: B) -> impl Future<Output = bool> + Send
	where
		B: Future<Output = bool> + Send,
		Self: Sized;
}

impl<Fut> BoolExt for Fut
where
	Fut: Future<Output = bool> + Send,
{
	#[inline]
	fn and<B>(self, b: B) -> impl Future<Output = bool> + Send
	where
		B: Future<Output = bool> + Send,
		Self: Sized,
	{
		type Result = crate::Result<(), ()>;

		let a = self.map(|a| a.then_some(()).ok_or(Result::Err(())));

		let b = b.map(|b| b.then_some(()).ok_or(Result::Err(())));

		try_join(a, b).map(|result| result.is_ok())
	}

	#[inline]
	fn or<B>(self, b: B) -> impl Future<Output = bool> + Send
	where
		B: Future<Output = bool> + Send,
		Self: Sized,
	{
		type Result = crate::Result<(), ()>;

		let a = self.map(|a| (!a).then_some(()).ok_or(Result::Err(())));

		let b = b.map(|b| (!b).then_some(()).ok_or(Result::Err(())));

		try_join(a, b).map(|result| result.is_err())
	}
}

pub fn and<I, F>(args: I) -> impl Future<Output = bool> + Send
where
	I: Iterator<Item = F> + Send,
	F: Future<Output = bool> + Send,
{
	type Result = crate::Result<(), ()>;

	let args = args.map(|a| a.map(|a| a.then_some(()).ok_or(Result::Err(()))));

	try_join_all(args).map(|result| result.is_ok())
}

pub fn or<I, F>(args: I) -> impl Future<Output = bool> + Send
where
	I: Iterator<Item = F> + Send,
	F: Future<Output = bool> + Send,
{
	type Result = crate::Result<(), ()>;

	let args = args.map(|a| a.map(|a| (!a).then_some(()).ok_or(Result::Err(()))));

	try_join_all(args).map(|result| result.is_err())
}

#[cfg(test)]
mod tests {
	use std::pin::pin;

	use futures::future;

	use super::{BoolExt, and, or};

	#[tokio::test]
	async fn bool_ext_truth_tables() {
		for a in [false, true] {
			for b in [false, true] {
				assert_eq!(future::ready(a).and(future::ready(b)).await, a && b, "and({a}, {b})");
				assert_eq!(future::ready(a).or(future::ready(b)).await, a || b, "or({a}, {b})");
			}
		}

		assert!(
			future::ready(false)
				.or(future::ready(false))
				.or(future::ready(true))
				.await,
			"chained or with trailing true must return true"
		);
		assert!(
			!future::ready(false)
				.or(future::ready(false))
				.or(future::ready(false))
				.await,
			"chained or with all false must return false"
		);
	}

	#[tokio::test]
	async fn bool_ext_short_circuits() {
		let pend = pin!(future::pending::<bool>());
		assert!(
			future::ready(true).or(pend).await,
			"or must short-circuit on true without polling pending"
		);

		let pend = pin!(future::pending::<bool>());
		assert!(
			!future::ready(false).and(pend).await,
			"and must short-circuit on false without polling pending"
		);
	}

	#[tokio::test]
	async fn free_and_or() {
		assert!(and(std::iter::empty::<future::Ready<bool>>()).await, "empty and is true");
		assert!(!or(std::iter::empty::<future::Ready<bool>>()).await, "empty or is false");

		assert!(and([future::ready(true), future::ready(true)].into_iter()).await, "all true");
		assert!(
			!and([future::ready(true), future::ready(false)].into_iter()).await,
			"and with false"
		);
		assert!(
			or([future::ready(false), future::ready(true)].into_iter()).await,
			"or with true"
		);
		assert!(!or([future::ready(false), future::ready(false)].into_iter()).await, "all false");
	}
}
