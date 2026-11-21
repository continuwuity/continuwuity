use std::convert::Infallible;

use super::Result;

pub trait UnwrapInfallible<T> {
	fn unwrap_infallible(self) -> T;
}

impl<T> UnwrapInfallible<T> for Result<T, Infallible> {
	#[inline]
	fn unwrap_infallible(self) -> T {
		let Ok(val) = self;
		val
	}
}
