mod err;
mod log;
mod panic;
mod response;
mod serde;

use std::{any::Any, borrow::Cow, convert::Infallible, sync::PoisonError};

use snafu::{IntoError, prelude::*};

pub use self::{err::visit, log::*};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum Error {
	#[snafu(display("PANIC!"))]
	PanicAny {
		panic: Box<dyn Any + Send>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("PANIC! {message}"))]
	Panic {
		message: &'static str,
		panic: Box<dyn Any + Send + 'static>,
		backtrace: snafu::Backtrace,
	},

	// std
	#[snafu(display("Format error: {source}"))]
	Fmt {
		source: std::fmt::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("UTF-8 conversion error: {source}"))]
	FromUtf8 {
		source: std::string::FromUtf8Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("I/O error: {source}"))]
	Io {
		source: std::io::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Parse float error: {source}"))]
	ParseFloat {
		source: std::num::ParseFloatError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Parse int error: {source}"))]
	ParseInt {
		source: std::num::ParseIntError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Error: {source}"))]
	Std {
		source: Box<dyn std::error::Error + Send>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Thread access error: {source}"))]
	ThreadAccessError {
		source: std::thread::AccessError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Integer conversion error: {source}"))]
	TryFromInt {
		source: std::num::TryFromIntError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Slice conversion error: {source}"))]
	TryFromSlice {
		source: std::array::TryFromSliceError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("UTF-8 error: {source}"))]
	Utf8 {
		source: std::str::Utf8Error,
		backtrace: snafu::Backtrace,
	},

	// third-party
	#[snafu(display("Capacity error: {source}"))]
	CapacityError {
		source: arrayvec::CapacityError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Cargo.toml error: {source}"))]
	CargoToml {
		source: cargo_toml::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Clap error: {source}"))]
	Clap {
		source: clap::error::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Extension rejection: {source}"))]
	Extension {
		source: axum::extract::rejection::ExtensionRejection,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Figment error: {source}"))]
	Figment {
		source: figment::error::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("HTTP error: {source}"))]
	Http {
		source: http::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Invalid HTTP header value: {source}"))]
	HttpHeader {
		source: http::header::InvalidHeaderValue,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Join error: {source}"))]
	JoinError {
		source: tokio::task::JoinError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("JSON error: {source}"))]
	Json {
		source: serde_json::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("JS parse int error: {source}"))]
	JsParseInt {
		source: ruma::JsParseIntError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("JS try from int error: {source}"))]
	JsTryFromInt {
		source: ruma::JsTryFromIntError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Path rejection: {source}"))]
	Path {
		source: axum::extract::rejection::PathRejection,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Mutex poisoned: {message}"))]
	Poison {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Regex error: {source}"))]
	Regex {
		source: regex::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Request error: {source}"))]
	Reqwest {
		source: reqwest::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{message}"))]
	SerdeDe {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{message}"))]
	SerdeSer {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("TOML deserialization error: {source}"))]
	TomlDe {
		source: toml::de::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("TOML serialization error: {source}"))]
	TomlSer {
		source: toml::ser::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Tracing filter error: {source}"))]
	TracingFilter {
		source: tracing_subscriber::filter::ParseError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Tracing reload error: {source}"))]
	TracingReload {
		source: tracing_subscriber::reload::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Typed header rejection: {source}"))]
	TypedHeader {
		source: axum_extra::typed_header::TypedHeaderRejection,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("YAML deserialization error: {source}"))]
	YamlDe {
		source: serde_saphyr::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("YAML serialization error: {source}"))]
	YamlSer {
		source: serde_saphyr::ser_error::Error,
		backtrace: snafu::Backtrace,
	},

	// ruma/conduwuit
	#[snafu(display("Arithmetic operation failed: {message}"))]
	Arithmetic {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{kind}: {message}"))]
	BadRequest {
		kind: ruma::api::client::error::ErrorKind,
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{message}"))]
	BadServerResponse {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Canonical JSON error: {source}"))]
	CanonicalJson {
		source: ruma::CanonicalJsonError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display(
		"There was a problem with the '{directive}' directive in your configuration: {message}"
	))]
	Config {
		directive: &'static str,
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{message}"))]
	Conflict {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Content disposition error: {source}"))]
	ContentDisposition {
		source: ruma::http_headers::ContentDispositionParseError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{message}"))]
	Database {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Feature '{feature}' is not available on this server."))]
	FeatureDisabled {
		feature: Cow<'static, str>,
	},

	#[snafu(display("Remote server {server} responded with: {error}"))]
	Federation {
		server: ruma::OwnedServerName,
		error: ruma::api::client::error::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{message} in {room_id}"))]
	InconsistentRoomState {
		message: &'static str,
		room_id: ruma::OwnedRoomId,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("HTTP conversion error: {source}"))]
	IntoHttp {
		source: ruma::api::error::IntoHttpError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{message}"))]
	Ldap {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("MXC URI error: {source}"))]
	Mxc {
		source: ruma::MxcUriError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Matrix ID parse error: {source}"))]
	Mxid {
		source: ruma::IdParseError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("from {server}: {error}"))]
	Redaction {
		server: ruma::OwnedServerName,
		error: ruma::canonical_json::RedactionError,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("{kind}: {message}"))]
	Request {
		kind: ruma::api::client::error::ErrorKind,
		message: Cow<'static, str>,
		code: http::StatusCode,
		backtrace: Option<snafu::Backtrace>,
	},

	#[snafu(display("Ruma error: {source}"))]
	Ruma {
		source: ruma::api::client::error::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("Signature error: {source}"))]
	Signatures {
		source: ruma::signatures::Error,
		backtrace: snafu::Backtrace,
	},

	#[snafu(display("State resolution error: {source}"))]
	#[snafu(context(false))]
	StateRes {
		source: crate::state_res::Error,
	},

	#[snafu(display("uiaa"))]
	Uiaa {
		info: ruma::api::client::uiaa::UiaaInfo,
	},

	// unique / untyped
	#[snafu(display("{message}"))]
	Err {
		message: Cow<'static, str>,
		backtrace: snafu::Backtrace,
	},
}

impl Error {
	#[inline]
	#[must_use]
	pub fn from_errno() -> Self { IoSnafu {}.into_error(std::io::Error::last_os_error()) }

	//#[deprecated]
	#[must_use] 
	pub fn bad_database(message: &'static str) -> Self {
		let message: Cow<'static, str> = message.into();
		DatabaseSnafu { message }.build()
	}

	/// Sanitizes public-facing errors that can leak sensitive information.
	pub fn sanitized_message(&self) -> String {
		match self {
			| Self::Database { .. } => String::from("Database error occurred."),
			| Self::Io { .. } => String::from("I/O error occurred."),
			| _ => self.message(),
		}
	}

	/// Generate the error message string.
	pub fn message(&self) -> String {
		match self {
			| Self::Federation { server, error, .. } => format!("Answer from {server}: {error}"),
			| Self::Ruma { source, .. } => response::ruma_error_message(source),
			| _ => format!("{self}"),
		}
	}

	/// Returns the Matrix error code / error kind
	#[inline]
	pub fn kind(&self) -> ruma::api::client::error::ErrorKind {
		use ruma::api::client::error::ErrorKind::{FeatureDisabled, Unknown};

		match self {
			| Self::Federation { error, .. } => response::ruma_error_kind(error).clone(),
			| Self::Ruma { source, .. } => response::ruma_error_kind(source).clone(),
			| Self::BadRequest { kind, .. } | Self::Request { kind, .. } => kind.clone(),
			| Self::FeatureDisabled { .. } => FeatureDisabled,
			| _ => Unknown,
		}
	}

	/// Returns the HTTP error code or closest approximation based on error
	/// variant.
	pub fn status_code(&self) -> http::StatusCode {
		use http::StatusCode;

		match self {
			| Self::Federation { error, .. } => error.status_code,
			| Self::Ruma { source, .. } => source.status_code,
			| Self::Request { kind, code, .. } => response::status_code(kind, *code),
			| Self::BadRequest { kind, .. } => response::bad_request_code(kind),
			| Self::FeatureDisabled { .. } => response::bad_request_code(&self.kind()),
			| Self::Reqwest { source, .. } =>
				source.status().unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
			| Self::Conflict { .. } => StatusCode::CONFLICT,
			| Self::Io { source, .. } => response::io_error_code(source.kind()),
			| _ => StatusCode::INTERNAL_SERVER_ERROR,
		}
	}

	/// Returns true for "not found" errors. This means anything that qualifies
	/// as a "not found" from any variant's contained error type. This call is
	/// often used as a special case to eliminate a contained Option with a
	/// Result where Ok(None) is instead Err(e) if e.is_not_found().
	#[inline]
	pub fn is_not_found(&self) -> bool { self.status_code() == http::StatusCode::NOT_FOUND }
}

// Debug is already derived by Snafu

/// Macro to reduce boilerplate for From implementations using Snafu context
macro_rules! impl_from_snafu {
	($source_ty:ty => $context:ident) => {
		impl From<$source_ty> for Error {
			fn from(source: $source_ty) -> Self { $context.into_error(source) }
		}
	};
}

/// Macro for From impls that format messages into ErrSnafu or other
/// message-based contexts
macro_rules! impl_from_message {
	($source_ty:ty => $context:ident, $msg:expr) => {
		impl From<$source_ty> for Error {
			fn from(source: $source_ty) -> Self {
				let message: Cow<'static, str> = format!($msg, source).into();
				$context { message }.build()
			}
		}
	};
}

/// Macro for From impls with constant messages (no formatting)
macro_rules! impl_from_const_message {
	($source_ty:ty => $context:ident, $msg:expr) => {
		impl From<$source_ty> for Error {
			fn from(_source: $source_ty) -> Self {
				let message: Cow<'static, str> = $msg.into();
				$context { message }.build()
			}
		}
	};
}

impl<T> From<PoisonError<T>> for Error {
	#[cold]
	#[inline(never)]
	fn from(e: PoisonError<T>) -> Self { PoisonSnafu { message: e.to_string() }.build() }
}

#[allow(clippy::fallible_impl_from)]
impl From<Infallible> for Error {
	#[cold]
	#[inline(never)]
	fn from(_e: Infallible) -> Self {
		panic!("infallible error should never exist");
	}
}

// Implementations using the macro
impl_from_snafu!(std::io::Error => IoSnafu);
impl_from_snafu!(std::string::FromUtf8Error => FromUtf8Snafu);
impl_from_snafu!(regex::Error => RegexSnafu);
impl_from_snafu!(ruma::http_headers::ContentDispositionParseError => ContentDispositionSnafu);
impl_from_snafu!(ruma::api::error::IntoHttpError => IntoHttpSnafu);
impl_from_snafu!(ruma::JsTryFromIntError => JsTryFromIntSnafu);
impl_from_snafu!(ruma::CanonicalJsonError => CanonicalJsonSnafu);
impl_from_snafu!(axum::extract::rejection::PathRejection => PathSnafu);
impl_from_snafu!(clap::error::Error => ClapSnafu);
impl_from_snafu!(ruma::MxcUriError => MxcSnafu);
impl_from_snafu!(serde_saphyr::ser_error::Error => YamlSerSnafu);
impl_from_snafu!(toml::de::Error => TomlDeSnafu);
impl_from_snafu!(http::header::InvalidHeaderValue => HttpHeaderSnafu);
impl_from_snafu!(serde_json::Error => JsonSnafu);

// Custom implementations using message formatting
impl_from_const_message!(std::fmt::Error => ErrSnafu, "formatting error");
impl_from_message!(std::str::Utf8Error => ErrSnafu, "UTF-8 error: {}");
impl_from_message!(std::num::TryFromIntError => ArithmeticSnafu, "integer conversion error: {}");
impl_from_message!(tracing_subscriber::reload::Error => ErrSnafu, "tracing reload error: {}");
impl_from_message!(reqwest::Error => ErrSnafu, "HTTP client error: {}");
impl_from_message!(ruma::signatures::Error => ErrSnafu, "Signature error: {}");
impl_from_message!(ruma::IdParseError => ErrSnafu, "ID parse error: {}");
impl_from_message!(std::num::ParseIntError => ErrSnafu, "Integer parse error: {}");
impl_from_message!(std::array::TryFromSliceError => ErrSnafu, "Slice conversion error: {}");
impl_from_message!(tokio::task::JoinError => ErrSnafu, "Task join error: {}");
impl_from_message!(serde_saphyr::Error => ErrSnafu, "YAML error: {}");

// Generic implementation for CapacityError
impl<T> From<arrayvec::CapacityError<T>> for Error {
	fn from(_source: arrayvec::CapacityError<T>) -> Self {
		let message: Cow<'static, str> = "capacity error: buffer is full".into();
		ErrSnafu { message }.build()
	}
}

#[cold]
#[inline(never)]
pub fn infallible(_e: &Infallible) {
	panic!("infallible error should never exist");
}

/// Convenience functor for fundamental Error::sanitized_message(); see member.
#[inline]
#[must_use]
#[allow(clippy::needless_pass_by_value)]
pub fn sanitized_message(e: Error) -> String { e.sanitized_message() }
