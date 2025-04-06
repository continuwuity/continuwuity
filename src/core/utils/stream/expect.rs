use futures::{Stream, StreamExt, TryStream};

use crate::Result;

pub trait TryExpect<'a, Item> {
	fn expect_ok(self) -> impl Stream<Item = Item> + Send + 'a;

	fn map_expect(self, msg: &'a str) -> impl Stream<Item = Item> + Send + 'a;
}

impl<'a, T, Item> TryExpect<'a, Item> for T
where
	T: Stream<Item = Result<Item>> + Send + TryStream + 'a,
	Item: 'a,
{
	#[inline]
	fn expect_ok(self: T) -> impl Stream<Item = Item> + Send + 'a {
		self.map_expect("stream expectation failure")
	}

	//TODO: move to impl MapExpect
	#[inline]
	fn map_expect(self, msg: &'a str) -> impl Stream<Item = Item> + Send + 'a {
		self.map(|res| res.expect(msg))
	}
}
