use std::{fmt, net::SocketAddr};

use conduwuit::{arrayvec::ArrayString, utils::math::Expected};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum FedDest {
	Literal(SocketAddr),       // "ip:port"
	Named(String, PortString), // ("hostname", ":port")
}

/// numeric or service-name
pub type PortString = ArrayString<16>;

const DEFAULT_PORT: &str = ":8448";

impl FedDest {
	pub(crate) fn uri_string(&self) -> String {
		match self {
			| Self::Literal(addr) => addr.to_string(),
			| Self::Named(host, port) => format!("{host}{port}"),
		}
	}

	#[inline]
	#[must_use]
	pub fn default_port() -> PortString {
		PortString::from(DEFAULT_PORT).expect("default port string")
	}

	#[inline]
	#[must_use]
	pub fn size(&self) -> usize {
		match self {
			| Self::Literal(saddr) => size_of_val(saddr),
			| Self::Named(host, port) => host.len().expected_add(port.capacity()),
		}
	}
}

impl fmt::Display for FedDest {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.uri_string().as_str())
	}
}
