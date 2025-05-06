type Delim<'a> = (&'a str, &'a str);

/// Slice a string between a pair of delimiters.
pub trait Between<'a> {
	/// Extract a string between the delimiters. If the delimiters were not
	/// found None is returned, otherwise the first extraction is returned.
	fn between(&self, delim: Delim<'_>) -> Option<&'a str>;

	/// Extract a string between the delimiters. If the delimiters were not
	/// found the original string is returned; take note of this behavior,
	/// if an empty slice is desired for this case use the fallible version and
	/// unwrap to EMPTY.
	fn between_infallible(&self, delim: Delim<'_>) -> &'a str;
}

impl<'a> Between<'a> for &'a str {
	#[inline]
	fn between_infallible(&self, delim: Delim<'_>) -> &'a str {
		self.between(delim).unwrap_or(self)
	}

	#[inline]
	fn between(&self, delim: Delim<'_>) -> Option<&'a str> {
		self.split_once(delim.0)
			.and_then(|(_, b)| b.rsplit_once(delim.1))
			.map(|(a, _)| a)
	}
}
