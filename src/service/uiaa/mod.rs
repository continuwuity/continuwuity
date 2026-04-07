use std::{
	borrow::Cow,
	collections::{HashMap, HashSet, hash_map::Entry},
	sync::Arc,
};

use conduwuit::{Err, Error, Result, error, utils, utils::hash};
use lettre::Address;
use ruma::{
	UserId,
	api::{
		client::uiaa::{
			AuthData, AuthFlow, AuthType, EmailIdentity, Password, ReCaptcha, RegistrationToken,
			ThirdpartyIdCredentials, UiaaInfo, UserIdentifier, MatrixUserIdentifier
		},
		error::{ErrorKind, StandardErrorBody},
	},
};
use serde_json::value::RawValue;
use tokio::sync::Mutex;

use crate::{Dep, config, globals, registration_tokens, rooms::user, threepid, users};

pub struct Service {
	services: Services,
	uiaa_sessions: Mutex<HashMap<String, UiaaSession>>,
}

struct Services {
	globals: Dep<globals::Service>,
	users: Dep<users::Service>,
	config: Dep<config::Service>,
	registration_tokens: Dep<registration_tokens::Service>,
	threepid: Dep<threepid::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				users: args.depend::<users::Service>("users"),
				config: args.depend::<config::Service>("config"),
				registration_tokens: args
					.depend::<registration_tokens::Service>("registration_tokens"),
				threepid: args.depend::<threepid::Service>("threepid"),
			},
			uiaa_sessions: Mutex::new(HashMap::new()),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

struct UiaaSession {
	info: UiaaInfo,
	identity: Identity,
}

/// Information about the authenticated user's identity.
///
/// A field of this struct will only be Some if the user completed
/// a stage which provided that information. If multiple stages provide
/// the same field, authentication will fail if they do not all provide
/// _identical_ values for that field.
#[derive(Default, Clone)]
pub struct Identity {
	/// The authenticated user's user ID, if it could be determined.
	///
	/// This will be Some if:
	/// - The user completed a m.login.password stage
	/// - The user completed a m.login.email.identity stage, and their email has
	///   an associated user ID
	pub localpart: Option<String>,

	/// The authenticated user's email address, if it could be determined.
	///
	/// This will be Some if:
	/// - The user completed a m.login.email.identity stage
	/// - The user completed a m.login.password stage, and their user ID has an
	///   associated email
	pub email: Option<Address>,
}

macro_rules! identity_update_fn {
	(fn $method:ident($field:ident : $type:ty)else $error:literal) => {
		fn $method(&mut self, $field: $type) -> Result<(), StandardErrorBody> {
			if self.$field.is_none() {
				self.$field = Some($field);
				Ok(())
			} else if self.$field == Some($field) {
				Ok(())
			} else {
				Err(StandardErrorBody {
					kind: ErrorKind::InvalidParam,
					message: $error.to_owned(),
				})
			}
		}
	};
}

impl Identity {
	identity_update_fn!(fn try_set_localpart(localpart: String) else "User ID mismatch");

	identity_update_fn!(fn try_set_email(email: Address) else "Email mismatch");

	/// Create an Identity with the localpart of the provided user ID
	/// and all other fields set to None.
	#[must_use]
	pub fn from_user_id(user_id: &UserId) -> Self {
		Self {
			localpart: Some(user_id.localpart().to_owned()),
			..Default::default()
		}
	}
}

impl Service {
	const SESSION_ID_LENGTH: usize = 32;

	/// Perform the full UIAA authentication sequence for a route given its
	/// authentication data.
	pub async fn authenticate(
		&self,
		auth: &Option<AuthData>,
		flows: Vec<AuthFlow>,
		params: Box<RawValue>,
		identity: Option<Identity>,
	) -> Result<Identity> {
		match auth.as_ref() {
			| None => {
				let info = self.create_session(flows, params, identity).await;

				Err(Error::Uiaa(info))
			},
			| Some(auth) => {
				let session: Cow<'_, str> = match auth.session() {
					| Some(session) => session.into(),
					| None => {
						// Clients are allowed to send UIAA requests with an auth dict and no
						// session if they want to start the UIAA exchange with existing
						// authentication data. If that happens, we create a new session
						// here.
						self.create_session(flows, params, identity)
							.await
							.session
							.unwrap()
							.into()
					},
				};

				match self.continue_session(auth, &session).await? {
					| Ok(identity) => Ok(identity),
					| Err(info) => Err(Error::Uiaa(info)),
				}
			},
		}
	}

	/// A helper to perform UIAA authentication with just a password stage.
	#[inline]
	pub async fn authenticate_password(
		&self,
		auth: &Option<AuthData>,
		identity: Option<Identity>,
	) -> Result<Identity> {
		self.authenticate(
			auth,
			vec![AuthFlow::new(vec![AuthType::Password])],
			Box::default(),
			identity,
		)
		.await
	}

	/// Create a new UIAA session with a random session ID.
	///
	/// If information about the user's identity is already known, it may be
	/// supplied with the `identity` parameter. Authentication will fail if
	/// flows provide different values for known identity information.
	///
	/// Returns the info of the newly created session.
	async fn create_session(
		&self,
		flows: Vec<AuthFlow>,
		params: Box<RawValue>,
		identity: Option<Identity>,
	) -> UiaaInfo {
		let mut uiaa_sessions = self.uiaa_sessions.lock().await;

		let session_id = utils::random_string(Self::SESSION_ID_LENGTH);
		let mut info = assign::assign!(UiaaInfo::new(flows), {params: Some(params)});
		info.session = Some(session_id.clone());

		uiaa_sessions.insert(session_id, UiaaSession {
			info: info.clone(),
			identity: identity.unwrap_or_default(),
		});

		info
	}

	/// Proceed with UIAA authentication given a client's authorization data.
	async fn continue_session(
		&self,
		auth: &AuthData,
		session: &str,
	) -> Result<Result<Identity, UiaaInfo>> {
		// Hold this lock for the entire function to make sure that, if try_auth()
		// is called concurrently with the same session, only one call will succeed
		let mut uiaa_sessions = self.uiaa_sessions.lock().await;

		let Entry::Occupied(mut session) = uiaa_sessions.entry(session.to_owned()) else {
			return Err!(Request(InvalidParam("Invalid session")));
		};

		if let &AuthData::FallbackAcknowledgement(_) = auth {
			// The client is checking if authentication has succeeded out-of-band. This is
			// possible if the client is using "fallback auth" (see spec section
			// 4.9.1.4), which we don't support (and probably never will, because it's a
			// disgusting hack).

			// Return early to tell the client that no, authentication did not succeed while
			// it wasn't looking.
			return Ok(Err(session.get().info.clone()));
		}

		let completed = {
			let UiaaSession { info, identity } = session.get_mut();

			let auth_type = auth.auth_type().expect("auth type should be set");

			let flow_stages: Vec<HashSet<_>> = info
				.flows
				.iter()
				.map(|flow| {
					flow.stages
						.iter()
						.map(AuthType::as_str)
						.map(ToOwned::to_owned)
						.collect()
				})
				.collect();

			let mut completed_stages: HashSet<_> = info
				.completed
				.iter()
				.map(AuthType::as_str)
				.map(ToOwned::to_owned)
				.collect();

			// Don't allow stages which aren't in any flows
			if !flow_stages
				.iter()
				.any(|stages| stages.contains(auth_type.as_str()))
			{
				return Err!(Request(InvalidParam("No flows include the supplied stage")));
			}

			// If the provided stage hasn't already been completed, check it for completion
			if !completed_stages.contains(auth_type.as_str()) {
				match self.check_stage(auth, identity.clone()).await {
					| Ok((completed_stage, updated_identity)) => {
						info.auth_error = None;
						completed_stages.insert(completed_stage.to_string());
						info.completed.push(completed_stage);
						*identity = updated_identity;
					},
					| Err(error) => {
						info.auth_error = Some(error);
					},
				}
			}

			// UIAA is completed if all stages in any flow are completed
			flow_stages
				.iter()
				.any(|stages| completed_stages.is_superset(stages))
		};

		if completed {
			// This session is complete, remove it and return success
			let (_, UiaaSession { identity, .. }) = session.remove_entry();

			Ok(Ok(identity))
		} else {
			// The client needs to try again, return the updated session
			Ok(Err(session.get().info.clone()))
		}
	}

	/// Check if the provided authentication data is valid.
	///
	/// Returns the completed stage's type on success and error information on
	/// failure.
	async fn check_stage(
		&self,
		auth: &AuthData,
		mut identity: Identity,
	) -> Result<(AuthType, Identity), StandardErrorBody> {
		// Note: This function takes ownership of `identity` because mutations to the
		// identity must not be applied unless checking the stage succeeds. The
		// updated identity is returned as part of the Ok value, and
		// `continue_session` handles saving it to `uiaa_sessions`.
		//
		// This also means it's fine to mutate `identity` at any point in this function,
		// because those mutations won't be saved unless the function returns Ok.

		match auth {
			| AuthData::Dummy(_) => Ok(AuthType::Dummy),
			| AuthData::EmailIdentity(EmailIdentity {
				thirdparty_id_creds: ThirdpartyIdCredentials { client_secret, sid, .. },
				..
			}) => {
				match self
					.services
					.threepid
					.consume_valid_session(sid, client_secret)
					.await
				{
					| Ok(email) => {
						if let Some(localpart) =
							self.services.threepid.get_localpart_for_email(&email).await
						{
							identity.try_set_localpart(localpart)?;
						}

						identity.try_set_email(email)?;

						Ok(AuthType::EmailIdentity)
					},
					| Err(message) => Err(StandardErrorBody {
						kind: ErrorKind::ThreepidAuthFailed,
						message: message.into_owned(),
					}),
				}
			},
			#[allow(clippy::useless_let_if_seq)]
			| AuthData::Password(Password { identifier, password, .. }) => {
				let user_id_or_localpart = match identifier {
					| Some(UserIdentifier::Matrix(MatrixUserIdentifier{user})) => user.to_owned(),
					| Some(UserIdentifier::Email(address)) => {
						let Ok(email) = Address::try_from(address.to_owned()) else {
							return Err(StandardErrorBody::new(ErrorKind::InvalidParam, "Email is malformed".to_owned()));
						};

						if let Some(localpart) =
							self.services.threepid.get_localpart_for_email(&email).await
						{
							identity.try_set_email(email)?;

							localpart
						} else {
							return Err(StandardErrorBody::new(ErrorKind::Forbidden, "Invalid identifier or password".to_owned()));
						}
					},
					| _ =>
						return Err(StandardErrorBody::new(ErrorKind::Unrecognized, "Identifier type not recognized".to_owned())),
				};

				let Ok(user_id) = UserId::parse_with_server_name(
					user_id_or_localpart,
					self.services.globals.server_name(),
				) else {
					return Err(StandardErrorBody::new(ErrorKind::InvalidParam, "User ID is malformed".to_owned()));
				};

				// Check if password is correct
				let mut password_verified = false;

				// First try local password hash verification
				if let Ok(hash) = self.services.users.password_hash(&user_id).await {
					password_verified = hash::verify_password(password, &hash).is_ok();
				}

				// If local password verification failed, try LDAP authentication
				#[cfg(feature = "ldap")]
				if !password_verified && self.services.config.ldap.enable {
					// Search for user in LDAP to get their DN
					if let Ok(dns) = self.services.users.search_ldap(&user_id).await {
						if let Some((user_dn, _is_admin)) = dns.first() {
							// Try to authenticate with LDAP
							password_verified = self
								.services
								.users
								.auth_ldap(user_dn, password)
								.await
								.is_ok();
						}
					}
				}

				if password_verified {
					identity.try_set_localpart(user_id.localpart().to_owned())?;

					Ok(AuthType::Password)
				} else {
					Err(StandardErrorBody::new(ErrorKind::Forbidden, "Invalid identifier or password".to_owned()))
				}
			},
			| AuthData::ReCaptcha(ReCaptcha { response, .. }) => {
				let Some(ref private_site_key) = self.services.config.recaptcha_private_site_key
				else {
					return Err(StandardErrorBody::new(ErrorKind::Forbidden, "ReCaptcha is not configured".to_owned()));
				};

				match recaptcha_verify::verify_v3(private_site_key, response, None).await {
					| Ok(()) => Ok(AuthType::ReCaptcha),
					| Err(e) => {
						error!("ReCaptcha verification failed: {e:?}");
						Err(StandardErrorBody::new(ErrorKind::Forbidden, "ReCaptcha verification failed".to_owned()))
					},
				}
			},
			| AuthData::RegistrationToken(RegistrationToken { token, .. }) => {
				let token = token.trim().to_owned();

				if let Some(valid_token) = self
					.services
					.registration_tokens
					.validate_token(token)
					.await
				{
					self.services
						.registration_tokens
						.mark_token_as_used(valid_token);

					Ok(AuthType::RegistrationToken)
				} else {
					Err(StandardErrorBody::new(ErrorKind::Forbidden, "Invalid registration token".to_owned()))
				}
			},
			| AuthData::Terms(_) => Ok(AuthType::Terms),
			| _ => Err(StandardErrorBody::new(ErrorKind::Unrecognized, "Unsupported stage type".into())),
		}
		.map(|auth_type| (auth_type, identity))
	}
}
