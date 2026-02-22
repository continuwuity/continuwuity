use futures::{
	StreamExt, stream,
	stream::{Stream, TryStream},
};

pub trait IterStream<I: IntoIterator + Send> {
	/// Convert an Iterator into a Stream
	fn stream(self) -> impl Stream<Item = <I as IntoIterator>::Item> + Send;

	/// Convert an Iterator into a TryStream with a generic error type
	fn try_stream<E>(
		self,
	) -> impl TryStream<
		Ok = <I as IntoIterator>::Item,
		Error = E,
		Item = Result<<I as IntoIterator>::Item, E>,
	> + Send;
}

impl<I> IterStream<I> for I
where
	I: IntoIterator + Send,
	<I as IntoIterator>::IntoIter: Send,
{
	#[inline]
	fn stream(self) -> impl Stream<Item = <I as IntoIterator>::Item> + Send { stream::iter(self) }

	#[inline]
	fn try_stream<E>(
		self,
	) -> impl TryStream<
		Ok = <I as IntoIterator>::Item,
		Error = E,
		Item = Result<<I as IntoIterator>::Item, E>,
	> + Send {
		self.stream().map(Ok)
	}
}
