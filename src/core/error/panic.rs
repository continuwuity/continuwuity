use std::{
	any::Any,
	panic::{RefUnwindSafe, UnwindSafe, panic_any},
};

use super::Error;
use crate::debug;

impl UnwindSafe for Error {}
impl RefUnwindSafe for Error {}

impl Error {
	#[inline]
	pub fn panic(self) -> ! { panic_any(self.into_panic()) }

	#[must_use]
	#[inline]
	pub fn from_panic(e: Box<dyn Any + Send>) -> Self {
		use super::PanicSnafu;
		PanicSnafu { message: debug::panic_str(&e), panic: e }.build()
	}

	#[inline]
	pub fn into_panic(self) -> Box<dyn Any + Send + 'static> {
		match self {
			| Self::Panic { panic, .. } | Self::PanicAny { panic, .. } => panic,
			| Self::JoinError { source, .. } => source.into_panic(),
			| _ => Box::new(self),
		}
	}

	/// Get the panic message string.
	#[inline]
	pub fn panic_str(self) -> Option<&'static str> {
		self.is_panic()
			.then_some(debug::panic_str(&self.into_panic()))
	}

	/// Check if the Error is trafficking a panic object.
	#[inline]
	pub fn is_panic(&self) -> bool {
		match &self {
			| Self::Panic { .. } | Self::PanicAny { .. } => true,
			| Self::JoinError { source, .. } => source.is_panic(),
			| _ => false,
		}
	}
}
