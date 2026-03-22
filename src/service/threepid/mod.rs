use std::{
	borrow::Cow,
	collections::HashMap,
	sync::{Arc, Mutex},
};

use crate::{
	Args, Dep, config,
	mailer::{self, messages::MessageTemplate},
};

mod data;
use conduwuit::{Err, Result, utils};
use data::{Data, ValidationToken};
use database::Deserialized;
use lettre::{Address, message::Mailbox};

pub struct Service {
	db: Data,
	services: Services,
	send_attempts: Mutex<HashMap<(String, Address), usize>>,
}

struct Services {
	config: Dep<config::Service>,
	mailer: Dep<mailer::Service>,
}

impl crate::Service for Service {
	fn build(args: Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data::new(args.db),
			services: Services {
				config: args.depend("config"),
				mailer: args.depend("mailer"),
			},
			send_attempts: Mutex::new(HashMap::new()),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	const RANDOM_SID_LENGTH: usize = 16;
	const VALIDATION_URL_PATH: &str = "/_continuwuity/3pid/email/validate";

	/// Send a validation message to an email address.
	///
	/// Returns the validation session ID on success.
	#[allow(clippy::impl_trait_in_params)]
	pub async fn send_validation_email<Template: MessageTemplate>(
		&self,
		recipient: Address,
		prepare_body: impl FnOnce(String) -> Template,
		client_secret: &str,
		send_attempt: usize,
	) -> Result<String> {
		let Some(mailer) = self.services.mailer.mailer() else {
			return Err!("SMTP is not configured");
		};

		let (session_id, ValidationToken { token, .. }) =
			match self.db.get_session_by_secret(client_secret).await {
				// If a validation session already exists for this client secret, we can either
				// reuse it with a new token or return early because it's already valid.
				| Some(session) => {
					// If the existing session is already valid, don't send an email.
					if session.has_been_validated {
						return Ok(session.session_id);
					}

					let mut send_attempts = self.send_attempts.lock().unwrap();
					match send_attempts
						.get_mut(&(session.client_secret.clone(), session.email.clone()))
					{
						| Some(last_send_attempt) => {
							if send_attempt <= *last_send_attempt {
								// If the supplied send attempt isn't higher than the last one,
								// don't send an email.
								return Ok(session.session_id);
							}

							// Otherwise save the supplied send attempt.
							*last_send_attempt = send_attempt;
						},
						| None => {
							// Default to sending an email if no previous
							// attempt could be found. This can happen if
							// the server was restarted, which clears the send
							// attempt tracker.
						},
					}
					drop(send_attempts);

					// Create a new token for the existing session.
					let token = ValidationToken::new_random();
					self.db
						.update_session_validation_token(&session, token.clone());

					(session.session_id, token)
				},
				// If no session exists, create a new one.
				| None => {
					let session_id = utils::random_string(Self::RANDOM_SID_LENGTH);
					let token = ValidationToken::new_random();

					self.db.create_session(
						recipient.clone(),
						session_id.clone(),
						client_secret.to_owned(),
						token.clone(),
					);

					(session_id, token)
				},
			};

		let mut validation_url = self
			.services
			.config
			.get_client_domain()
			.join(Self::VALIDATION_URL_PATH)
			.unwrap();

		validation_url
			.query_pairs_mut()
			.append_pair("session_id", &session_id)
			.append_pair("client_secret", client_secret)
			.append_pair("token", &token);

		let recipient = Mailbox::new(None, recipient);
		let message = prepare_body(validation_url.to_string());

		mailer.send(recipient, message).await?;

		Ok(session_id)
	}

	pub async fn try_validate_session(
		&self,
		session_id: &str,
		client_secret: &str,
		supplied_token: &str,
	) -> Result<(), Cow<'static, str>> {
		let Some(session) = self.db.get_session(session_id).await else {
			return Err("Validation session does not exist".into());
		};

		if session.has_been_validated {
			return Ok(());
		}

		if session.client_secret != client_secret {
			return Err("Invalid client secret for session".into());
		}

		let token = self
			.db
			.get_session_validation_token(&session)
			.await
			.expect("valid session should have a token");
		if token != *supplied_token || !token.is_valid() {
			return Err("Validation token is invalid or expired, please request a new one".into());
		}

		self.db.mark_session_as_valid(session).await;

		Ok(())
	}

	pub async fn consume_valid_session(
		&self,
		session_id: &str,
		client_secret: &str,
	) -> Result<Address, Cow<'static, str>> {
		let Some(session) = self.db.get_session(session_id).await else {
			return Err("Validation session does not exist".into());
		};

		if session.client_secret == client_secret && session.has_been_validated {
			let email = session.email.clone();
			self.db.remove_session(session).await;
			Ok(email)
		} else {
			Err("Validation failed. Did you use the link that was sent to you?".into())
		}
	}

	/// Associate a localpart with an email address.
	pub fn associate_localpart_email(&self, localpart: &str, email: Address) {
		self.db.localpart_email.raw_put(localpart, &email);
		self.db.email_localpart.put_raw(email, localpart);
	}

	/// Given a localpart, remove its corresponding email address.
	///
	/// [`Self::get_localpart_for_email`] may be used if only the email is
	/// known.
	pub async fn disassociate_localpart_email(&self, localpart: &str) {
		let email = self
			.get_email_for_localpart(localpart)
			.await
			.expect("localpart has no email associated");
		self.db.localpart_email.remove(localpart);
		self.db.email_localpart.del(&email);
	}

	/// Get the email associated with a localpart, if one exists.
	pub async fn get_email_for_localpart(&self, localpart: &str) -> Option<Address> {
		self.db
			.localpart_email
			.get(localpart)
			.await
			.deserialized()
			.ok()
	}

	/// Get the localpart associated with an email, if one exists.
	pub async fn get_localpart_for_email(&self, email: &Address) -> Option<String> {
		self.db.email_localpart.qry(email).await.deserialized().ok()
	}
}
