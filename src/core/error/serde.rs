use std::fmt::Display;

use serde::{de, ser};

use crate::Error;

impl de::Error for Error {
	fn custom<T: Display + ToString>(msg: T) -> Self {
		let message: std::borrow::Cow<'static, str> = msg.to_string().into();
		super::SerdeDeSnafu { message }.build()
	}
}

impl ser::Error for Error {
	fn custom<T: Display + ToString>(msg: T) -> Self {
		let message: std::borrow::Cow<'static, str> = msg.to_string().into();
		super::SerdeSerSnafu { message }.build()
	}
}
