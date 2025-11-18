use std::fmt;

/// Debug-formats the given slice, but only up to the first `max_len` elements.
/// Any further elements are replaced by an ellipsis.
///
/// See also [`slice_truncated()`],
pub struct TruncatedSlice<'a, T> {
	inner: &'a [T],
	max_len: usize,
}

impl<T: fmt::Debug> fmt::Debug for TruncatedSlice<'_, T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if self.inner.len() <= self.max_len {
			write!(f, "{:?}", self.inner)
		} else {
			f.debug_list()
				.entries(&self.inner[..self.max_len])
				.entry(&"...")
				.finish()
		}
	}
}

/// See [`TruncatedSlice`]. Useful for `#[instrument]`:
///
/// ```
/// use conduwuit_core::utils::debug::slice_truncated;
///
/// #[tracing::instrument(fields(foos = slice_truncated(foos, 42)))]
/// fn bar(foos: &[&str]) {};
/// ```
pub fn slice_truncated<T: fmt::Debug>(
	slice: &[T],
	max_len: usize,
) -> tracing::field::DebugValue<TruncatedSlice<'_, T>> {
	tracing::field::debug(TruncatedSlice { inner: slice, max_len })
}
