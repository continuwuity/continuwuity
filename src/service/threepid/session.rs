use std::{
	collections::HashMap,
	time::{Duration, SystemTime},
};

use conduwuit::utils;
use lettre::Address;
use ruma::{ClientSecret, OwnedClientSecret, OwnedSessionId, SessionId};

#[derive(Default)]
pub(super) struct ValidationSessions {
	sessions: HashMap<OwnedSessionId, ValidationSession>,
	client_secrets: HashMap<OwnedClientSecret, OwnedSessionId>,
}

/// A pending or completed email validation session.
#[derive(Debug)]
pub(crate) struct ValidationSession {
	/// The session's ID
	pub session_id: OwnedSessionId,
	/// The client's supplied client secret
	pub client_secret: OwnedClientSecret,
	/// The email address which is being validated
	pub email: Address,
	/// The session's validation state
	pub validation_state: ValidationState,
}

/// The state of an email validation session.
#[derive(Debug)]
pub(crate) enum ValidationState {
	/// The session is waiting for this validation token to be provided
	Pending(ValidationToken),
	/// The session has been validated
	Validated,
}

#[derive(Clone, Debug)]
pub(crate) struct ValidationToken {
	pub token: String,
	pub issued_at: SystemTime,
}

impl ValidationToken {
	// one hour
	const MAX_TOKEN_AGE: Duration = Duration::from_secs(60 * 60);
	const RANDOM_TOKEN_LENGTH: usize = 16;

	pub(super) fn new_random() -> Self {
		Self {
			token: utils::random_string(Self::RANDOM_TOKEN_LENGTH),
			issued_at: SystemTime::now(),
		}
	}

	pub(crate) fn is_valid(&self) -> bool {
		let now = SystemTime::now();

		now.duration_since(self.issued_at)
			.is_ok_and(|duration| duration < Self::MAX_TOKEN_AGE)
	}
}

impl PartialEq<str> for ValidationToken {
	fn eq(&self, other: &str) -> bool { self.token == other }
}

impl ValidationSessions {
	const RANDOM_SID_LENGTH: usize = 16;

	#[must_use]
	pub(super) fn generate_session_id() -> OwnedSessionId {
		SessionId::parse(utils::random_string(Self::RANDOM_SID_LENGTH)).unwrap()
	}

	pub(super) fn create_session(
		&mut self,
		email: Address,
		client_secret: OwnedClientSecret,
	) -> &mut ValidationSession {
		let session = ValidationSession {
			session_id: Self::generate_session_id(),
			client_secret,
			email,
			validation_state: ValidationState::Pending(ValidationToken::new_random()),
		};

		self.client_secrets
			.insert(session.client_secret.clone(), session.session_id.clone());
		self.sessions
			.entry(session.session_id.clone())
			.insert_entry(session)
			.into_mut()
	}

	pub(super) fn get_session(
		&mut self,
		session_id: &SessionId,
	) -> Option<&mut ValidationSession> {
		self.sessions.get_mut(session_id)
	}

	pub(super) fn get_session_by_client_secret(
		&mut self,
		client_secret: &ClientSecret,
	) -> Option<&mut ValidationSession> {
		let session_id = self.client_secrets.get(client_secret)?;
		let session = self
			.sessions
			.get_mut(session_id)
			.expect("session should exist with session id");

		Some(session)
	}

	pub(super) fn remove_session(&mut self, session_id: &SessionId) -> ValidationSession {
		let session = self
			.sessions
			.remove(session_id)
			.expect("session ID should exist");

		self.client_secrets
			.remove(&session.client_secret)
			.expect("session should have an associated client secret");

		session
	}
}
