use std::{borrow::Cow, collections::HashMap, sync::Arc};

use conduwuit::{Err, Error, Result, result::FlatOk};
use database::{Deserialized, Map};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use lettre::{Address, message::Mailbox};
use nonzero_ext::nonzero;
use ruma::{
	ClientSecret, OwnedClientSecret, OwnedSessionId, SessionId,
	api::error::{ErrorKind, LimitExceededErrorData},
};

mod session;

use crate::{
	Args, Dep, config,
	mailer::{self, messages::MessageTemplate},
	threepid::session::{ValidationSessions, ValidationState, ValidationToken},
};

pub struct Service {
	db: Data,
	services: Services,
	sessions: tokio::sync::Mutex<ValidationSessions>,
	send_attempts: std::sync::Mutex<HashMap<(OwnedClientSecret, Address), usize>>,
	ratelimiter: DefaultKeyedRateLimiter<Address>,
}

pub enum EmailRequirement {
	/// Users may change their email, but cannot remove it entirely.
	Required,
	/// Users may change or remove their email.
	Optional,
	/// Users may not change their email at all.
	Unavailable,
}

impl EmailRequirement {
	#[must_use]
	pub fn may_change(&self) -> bool { matches!(self, Self::Required | Self::Optional) }

	#[must_use]
	pub fn may_remove(&self) -> bool { matches!(self, Self::Optional) }
}

struct Data {
	localpart_email: Arc<Map>,
	email_localpart: Arc<Map>,
}

struct Services {
	config: Dep<config::Service>,
	mailer: Dep<mailer::Service>,
}

impl crate::Service for Service {
	fn build(args: Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				email_localpart: args.db["email_localpart"].clone(),
				localpart_email: args.db["localpart_email"].clone(),
			},
			services: Services {
				config: args.depend("config"),
				mailer: args.depend("mailer"),
			},
			sessions: tokio::sync::Mutex::default(),
			send_attempts: std::sync::Mutex::default(),
			ratelimiter: RateLimiter::keyed(Self::EMAIL_RATELIMIT),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	// Each address gets two tickets to send an email, which refill at a rate of one
	// per ten minutes. This allows two emails to be sent at once without waiting
	// (in case the first one gets eaten), but requires a wait of at least ten
	// minutes before sending another.
	const EMAIL_RATELIMIT: Quota =
		Quota::per_minute(nonzero!(10_u32)).allow_burst(nonzero!(2_u32));
	const VALIDATION_URL_PATH: &str = "/_continuwuity/3pid/email/validate";

	/// Check if users are required to have an email address.
	pub fn email_requirement(&self) -> EmailRequirement {
		if let Some(smtp) = &self.services.config.smtp {
			if smtp.require_email_for_registration || smtp.require_email_for_token_registration {
				EmailRequirement::Required
			} else {
				EmailRequirement::Optional
			}
		} else {
			EmailRequirement::Unavailable
		}
	}

	/// Send a validation message to an email address.
	///
	/// Returns the validation session ID on success.
	#[allow(clippy::impl_trait_in_params)]
	pub async fn send_validation_email<Template: MessageTemplate>(
		&self,
		recipient: Mailbox,
		prepare_body: impl FnOnce(String) -> Template,
		client_secret: &ClientSecret,
		send_attempt: usize,
	) -> Result<OwnedSessionId> {
		let mailer = self.services.mailer.expect_mailer()?;
		let mut sessions = self.sessions.lock().await;

		let session = match sessions.get_session_by_client_secret(client_secret) {
			// If a validation session already exists for this client secret, we can either
			// reuse it with a new token or return early because it's already valid.
			| Some(session) => {
				match session.validation_state {
					| ValidationState::Validated => {
						// If the existing session is already valid, don't send an email.
						return Ok(session.session_id.clone());
					},
					| ValidationState::Pending(ref mut token) => {
						// Check ratelimiting for the target address.
						if self.ratelimiter.check_key(&recipient.email).is_err() {
							return Err(Error::BadRequest(
								ErrorKind::LimitExceeded(LimitExceededErrorData::new()),
								"You're sending emails too fast, try again in a few minutes.",
							));
						}

						// Check the send attempt for this session.
						let mut send_attempts = self.send_attempts.lock().unwrap();

						let last_send_attempt = send_attempts
							.entry((session.client_secret.clone(), session.email.clone()))
							.or_default();

						if send_attempt <= *last_send_attempt {
							// If the supplied send attempt isn't higher than the last
							// one, don't send an email.
							return Ok(session.session_id.clone());
						}

						// Save this send attempt.
						*last_send_attempt = send_attempt;
						drop(send_attempts);

						// Create a new token for the existing session.
						*token = ValidationToken::new_random();

						session
					},
				}
			},
			// If no session exists, create a new one.
			| None => sessions.create_session(recipient.email.clone(), client_secret.to_owned()),
		};

		// Clone this so it can outlive the lock we're holding on `sessions`
		let session_id = session.session_id.clone();

		let ValidationState::Pending(token) = &session.validation_state else {
			unreachable!("session should be pending")
		};

		let mut validation_url = self
			.services
			.config
			.get_client_domain()
			.join(Self::VALIDATION_URL_PATH)
			.unwrap();

		validation_url
			.query_pairs_mut()
			.append_pair("session", session_id.as_str())
			.append_pair("token", &token.token);

		// Once the validation URL is built, we don't need any data borrowed from
		// `sessions` anymore and can release our lock
		drop(sessions);

		let message = prepare_body(validation_url.to_string());

		mailer.send(recipient, message).await?;

		Ok(session_id)
	}

	/// Attempt to mark a validation session as valid using a validation token.
	pub async fn try_validate_session(
		&self,
		session_id: &SessionId,
		supplied_token: &str,
	) -> Result<(), Cow<'static, str>> {
		let mut sessions = self.sessions.lock().await;

		let Some(session) = sessions.get_session(session_id) else {
			return Err("Validation session does not exist".into());
		};

		session.validation_state = match &session.validation_state {
			| ValidationState::Validated => {
				// If the session is already validated, do nothing.

				return Ok(());
			},
			| ValidationState::Pending(token) => {
				// Otherwise check the token and mark the session as valid.

				if *token != *supplied_token || !token.is_valid() {
					return Err("Validation token is invalid or expired, please request a new \
					            one"
					.into());
				}

				ValidationState::Validated
			},
		};

		Ok(())
	}

	/// Consume a validated validation session, removing it from the database
	/// and returning the newly validated email address.
	pub async fn consume_valid_session(
		&self,
		session_id: &SessionId,
		client_secret: &ClientSecret,
	) -> Result<Address, Cow<'static, str>> {
		let mut sessions = self.sessions.lock().await;

		let Some(session) = sessions.get_session(session_id) else {
			return Err("Validation session does not exist".into());
		};

		if session.client_secret == client_secret
			&& matches!(session.validation_state, ValidationState::Validated)
		{
			let session = sessions.remove_session(session_id);

			Ok(session.email)
		} else {
			Err("This email address has not been validated. Did you use the link that was sent \
			     to you?"
				.into())
		}
	}

	/// Associate a localpart with an email address.
	pub async fn associate_localpart_email(
		&self,
		localpart: &str,
		email: &Address,
	) -> Result<()> {
		match self.get_localpart_for_email(email).await {
			| Some(existing_localpart) if existing_localpart != localpart => {
				// Another account is already using the supplied email.

				Err!(Request(ThreepidInUse("This email address is already in use.")))
			},
			| Some(_) => {
				// The supplied localpart is already associated with the supplied email,
				// no changes are necessary.
				Ok(())
			},
			| None => {
				// The supplied email is not already in use.

				// Remove the user's existing email first.
				let _ = self.disassociate_localpart_email(localpart).await;

				let email: &str = email.as_ref();
				self.db.localpart_email.insert(localpart, email);
				self.db.email_localpart.insert(email, localpart);
				Ok(())
			},
		}
	}

	/// Given a localpart, remove its corresponding email address.
	///
	/// [`Self::get_localpart_for_email`] may be used if only the email is
	/// known.
	pub async fn disassociate_localpart_email(&self, localpart: &str) -> Option<Address> {
		let email = self.get_email_for_localpart(localpart).await?;

		self.db.localpart_email.remove(localpart);
		self.db
			.email_localpart
			.remove(<Address as AsRef<str>>::as_ref(&email));

		Some(email)
	}

	/// Get the email associated with a localpart, if one exists.
	pub async fn get_email_for_localpart(&self, localpart: &str) -> Option<Address> {
		self.db
			.localpart_email
			.get(localpart)
			.await
			.deserialized::<String>()
			.ok()
			.map(TryInto::try_into)
			.flat_ok()
	}

	/// Get the localpart associated with an email, if one exists.
	pub async fn get_localpart_for_email(&self, email: &Address) -> Option<String> {
		self.db
			.email_localpart
			.get(<Address as AsRef<str>>::as_ref(email))
			.await
			.deserialized()
			.ok()
	}
}
