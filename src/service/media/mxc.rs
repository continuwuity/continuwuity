use std::fmt;

use ruma::{MxcUri, MxcUriError, OwnedMxcUri, ServerName};
use serde::{Serialize, Serializer};

/// A structured, valid MXC URI
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mxc<'a> {
	/// ServerName part of the MXC URI
	pub server_name: &'a ServerName,

	/// MediaId part of the MXC URI
	pub media_id: &'a str,
}

impl fmt::Display for Mxc<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "mxc://{}/{}", self.server_name, self.media_id)
	}
}

impl<'a> TryFrom<&'a MxcUri> for Mxc<'a> {
	type Error = MxcUriError;

	fn try_from(s: &'a MxcUri) -> Result<Self, Self::Error> {
		let (server_name, media_id) = s.parts()?;

		Ok(Self { server_name, media_id })
	}
}

impl<'a> TryFrom<&'a str> for Mxc<'a> {
	type Error = MxcUriError;

	fn try_from(s: &'a str) -> Result<Self, Self::Error> {
		let s: &MxcUri = s.into();
		s.try_into()
	}
}

impl<'a> TryFrom<&'a OwnedMxcUri> for Mxc<'a> {
	type Error = MxcUriError;

	fn try_from(s: &'a OwnedMxcUri) -> Result<Self, Self::Error> {
		let s: &MxcUri = s.as_ref();
		s.try_into()
	}
}

impl Serialize for Mxc<'_> {
	fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
		s.serialize_str(self.to_string().as_str())
	}
}
