use continuwuity::Result;
use ruma::{UserId, api::appservice::Registration};

use super::NamespaceRegex;

/// Appservice registration combined with its compiled regular expressions.
#[derive(Clone, Debug)]
pub struct RegistrationInfo {
	pub registration: Registration,
	pub users: NamespaceRegex,
	pub aliases: NamespaceRegex,
	pub rooms: NamespaceRegex,
}

impl RegistrationInfo {
	#[must_use]
	pub fn is_user_match(&self, user_id: &UserId) -> bool {
		self.users.is_match(user_id.as_str())
			|| self.registration.sender_localpart == user_id.localpart()
	}

	#[inline]
	#[must_use]
	pub fn is_exclusive_user_match(&self, user_id: &UserId) -> bool {
		self.users.is_exclusive_match(user_id.as_str())
			|| self.registration.sender_localpart == user_id.localpart()
	}
}

impl TryFrom<Registration> for RegistrationInfo {
	type Error = regex::Error;

	fn try_from(value: Registration) -> Result<Self, regex::Error> {
		Ok(Self {
			users: value.namespaces.users.clone().try_into()?,
			aliases: value.namespaces.aliases.clone().try_into()?,
			rooms: value.namespaces.rooms.clone().try_into()?,
			registration: value,
		})
	}
}
