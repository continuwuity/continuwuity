use serde_json::Error as JsonError;
use snafu::{IntoError, prelude::*};

/// Represents the various errors that arise when resolving state.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
	/// A deserialization error.
	#[snafu(display("JSON error: {source}"))]
	SerdeJson {
		source: JsonError,
		backtrace: snafu::Backtrace,
	},

	/// The given option or version is unsupported.
	#[snafu(display("Unsupported room version: {version}"))]
	Unsupported {
		version: String,
		backtrace: snafu::Backtrace,
	},

	/// The given event was not found.
	#[snafu(display("Not found error: {message}"))]
	NotFound {
		message: String,
		backtrace: snafu::Backtrace,
	},

	/// Invalid fields in the given PDU.
	#[snafu(display("Invalid PDU: {message}"))]
	InvalidPdu {
		message: String,
		backtrace: snafu::Backtrace,
	},
}

impl From<serde_json::Error> for Error {
	fn from(source: serde_json::Error) -> Self { SerdeJsonSnafu.into_error(source) }
}
