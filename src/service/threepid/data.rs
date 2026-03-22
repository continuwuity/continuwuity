use std::{
	sync::Arc,
	time::{Duration, SystemTime},
};

use conduwuit::utils;
use database::{Database, Deserialized, Map};
use lettre::Address;
use ruma::{ClientSecret, OwnedClientSecret, OwnedSessionId};
use serde::{Deserialize, Serialize};

pub(super) struct Data {
	// note: the column names of these maps use `validationsession` instead of `session`
	clientsecret_sessionid: Arc<Map>,
	sessionid_session: Arc<Map>,
	sessionid_token: Arc<Map>,
	pub(super) localpart_email: Arc<Map>,
	pub(super) email_localpart: Arc<Map>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ValidationSession {
	/// The session's ID
	pub session_id: OwnedSessionId,
	/// The email address which is being validated
	pub email: Address,
	/// The client's supplied client secret
	pub client_secret: OwnedClientSecret,
	/// Whether the email address has been validated successfully yet
	pub(super) has_been_validated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

impl Data {
	pub(super) fn new(db: &Arc<Database>) -> Self {
		Self {
			clientsecret_sessionid: db["clientsecret_validationsessionid"].clone(),
			sessionid_session: db["validationsessionid_session"].clone(),
			sessionid_token: db["validationsessionid_token"].clone(),
			localpart_email: db["localpart_email"].clone(),
			email_localpart: db["email_localpart"].clone(),
		}
	}

	/// Create a validation session.
	pub(super) fn create_session(
		&self,
		email: Address,
		session_id: OwnedSessionId,
		client_secret: OwnedClientSecret,
		token: ValidationToken,
	) {
		let session = ValidationSession {
			session_id,
			client_secret,
			email,
			has_been_validated: false,
		};
		self.clientsecret_sessionid
			.insert(&session.session_id, &session.client_secret);
		self.sessionid_token.raw_put(&session.session_id, token);
		self.sessionid_session
			.raw_put(session.session_id.clone(), session);
	}

	/// Get a validation session.
	pub(super) async fn get_session(&self, session_id: &str) -> Option<ValidationSession> {
		self.sessionid_session
			.get(session_id)
			.await
			.deserialized()
			.ok()
	}

	/// Get a validation session by client secret.
	pub(super) async fn get_session_by_secret(
		&self,
		client_secret: &ClientSecret,
	) -> Option<ValidationSession> {
		let session_id: String = self
			.clientsecret_sessionid
			.get(client_secret)
			.await
			.deserialized()
			.ok()?;

		self.get_session(&session_id).await
	}

	/// Get the validation token for a validation session, or None if the
	/// session does not exist.
	pub(super) async fn get_session_validation_token(
		&self,
		session: &ValidationSession,
	) -> Option<ValidationToken> {
		self.sessionid_token
			.get(&session.session_id)
			.await
			.deserialized()
			.ok()
	}

	/// Update a session's validation token.
	pub(super) fn update_session_validation_token(
		&self,
		session: &ValidationSession,
		token: ValidationToken,
	) {
		self.sessionid_token.raw_put(&session.session_id, token);
	}

	/// Mark a validation session as valid.
	pub(super) async fn mark_session_as_valid(&self, mut session: ValidationSession) {
		self.sessionid_token.remove(&session.session_id);

		session.has_been_validated = true;
		self.sessionid_session
			.raw_put(session.session_id.clone(), session);
	}

	/// Remove a validation session.
	pub(super) async fn remove_session(
		&self,
		ValidationSession { session_id, .. }: ValidationSession,
	) {
		self.clientsecret_sessionid.remove(&session_id);
		self.sessionid_token.remove(&session_id);
		self.sessionid_session.remove(&session_id);
	}
}
