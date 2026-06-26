use std::fmt::Debug;

use ruma::{OwnedServerName, OwnedUserId};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Destination {
	Appservice(String),
	Push(OwnedUserId, String), // user and pushkey
	Federation(OwnedServerName),
}

impl Destination {
	/// Gets the prefix for this destination.
	#[must_use]
	pub(super) fn get_prefix(&self) -> Vec<u8> {
		match self {
			| Self::Federation(server) => {
				let len = server.as_bytes().len().saturating_add(1);

				let mut p = Vec::with_capacity(len);
				p.extend_from_slice(server.as_bytes());
				p.push(0xFF);
				p
			},
			| Self::Appservice(server) => {
				let sigil = b"+";
				let len = sigil.len().saturating_add(server.len()).saturating_add(1);

				let mut p = Vec::with_capacity(len);
				p.extend_from_slice(sigil);
				p.extend_from_slice(server.as_bytes());
				p.push(0xFF);
				p
			},
			| Self::Push(user, pushkey) => {
				let sigil = b"$";
				let len = sigil
					.len()
					.saturating_add(user.as_bytes().len())
					.saturating_add(1)
					.saturating_add(pushkey.len())
					.saturating_add(1);

				let mut p = Vec::with_capacity(len);
				p.extend_from_slice(sigil);
				p.extend_from_slice(user.as_bytes());
				p.push(0xFF);
				p.extend_from_slice(pushkey.as_bytes());
				p.push(0xFF);
				p
			},
		}
	}
}
