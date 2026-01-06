use std::{collections::BTreeMap, sync::Arc};

use conduwuit::{
	Err, Error, Result, SyncRwLock, err, error, implement, utils,
	utils::{hash, string::EMPTY},
};
use database::{Deserialized, Json, Map};
use ruma::{
	CanonicalJsonValue, DeviceId, OwnedDeviceId, OwnedUserId, UserId,
	api::client::{
		error::{ErrorKind, StandardErrorBody},
		uiaa::{AuthData, AuthType, Password, UiaaInfo, UserIdentifier},
	},
};

use crate::{Dep, config, globals, registration_tokens, users};

pub struct Service {
	userdevicesessionid_uiaarequest: SyncRwLock<RequestMap>,
	db: Data,
	services: Services,
}

struct Services {
	globals: Dep<globals::Service>,
	users: Dep<users::Service>,
	config: Dep<config::Service>,
	registration_tokens: Dep<registration_tokens::Service>,
}

struct Data {
	userdevicesessionid_uiaainfo: Arc<Map>,
}

type RequestMap = BTreeMap<RequestKey, CanonicalJsonValue>;
type RequestKey = (OwnedUserId, OwnedDeviceId, String);

pub const SESSION_ID_LENGTH: usize = 32;

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			userdevicesessionid_uiaarequest: SyncRwLock::new(RequestMap::new()),
			db: Data {
				userdevicesessionid_uiaainfo: args.db["userdevicesessionid_uiaainfo"].clone(),
			},
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				users: args.depend::<users::Service>("users"),
				config: args.depend::<config::Service>("config"),
				registration_tokens: args
					.depend::<registration_tokens::Service>("registration_tokens"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

/// Creates a new Uiaa session. Make sure the session token is unique.
#[implement(Service)]
pub fn create(
	&self,
	user_id: &UserId,
	device_id: &DeviceId,
	uiaainfo: &UiaaInfo,
	json_body: &CanonicalJsonValue,
) {
	// TODO: better session error handling (why is uiaainfo.session optional in
	// ruma?)
	self.set_uiaa_request(
		user_id,
		device_id,
		uiaainfo.session.as_ref().expect("session should be set"),
		json_body,
	);

	self.update_uiaa_session(
		user_id,
		device_id,
		uiaainfo.session.as_ref().expect("session should be set"),
		Some(uiaainfo),
	);
}

#[implement(Service)]
#[allow(clippy::useless_let_if_seq)]
pub async fn try_auth(
	&self,
	user_id: &UserId,
	device_id: &DeviceId,
	auth: &AuthData,
	uiaainfo: &UiaaInfo,
) -> Result<(bool, UiaaInfo)> {
	let mut uiaainfo = if let Some(session) = auth.session() {
		self.get_uiaa_session(user_id, device_id, session).await?
	} else {
		uiaainfo.clone()
	};

	if uiaainfo.session.is_none() {
		uiaainfo.session = Some(utils::random_string(SESSION_ID_LENGTH));
	}

	match auth {
		// Find out what the user completed
		| AuthData::Password(Password {
			identifier,
			password,
			#[cfg(feature = "element_hacks")]
			user,
			..
		}) => {
			#[cfg(feature = "element_hacks")]
			let username = if let Some(UserIdentifier::UserIdOrLocalpart(username)) = identifier {
				username
			} else if let Some(username) = user {
				username
			} else {
				return Err(Error::BadRequest(
					ErrorKind::Unrecognized,
					"Identifier type not recognized.",
				));
			};

			#[cfg(not(feature = "element_hacks"))]
			let Some(UserIdentifier::UserIdOrLocalpart(username)) = identifier else {
				return Err(Error::BadRequest(
					ErrorKind::Unrecognized,
					"Identifier type not recognized.",
				));
			};

			let user_id_from_username = UserId::parse_with_server_name(
				username.clone(),
				self.services.globals.server_name(),
			)
			.map_err(|_| Error::BadRequest(ErrorKind::InvalidParam, "User ID is invalid."))?;

			// Check if the access token being used matches the credentials used for UIAA
			if user_id.localpart() != user_id_from_username.localpart() {
				return Err!(Request(Forbidden("User ID and access token mismatch.")));
			}
			let user_id = user_id_from_username;

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

			if !password_verified {
				uiaainfo.auth_error = Some(StandardErrorBody {
					kind: ErrorKind::forbidden(),
					message: "Invalid username or password.".to_owned(),
				});

				return Ok((false, uiaainfo));
			}

			// Password was correct! Let's add it to `completed`
			uiaainfo.completed.push(AuthType::Password);
		},
		| AuthData::ReCaptcha(r) => {
			if self.services.config.recaptcha_private_site_key.is_none() {
				return Err!(Request(Forbidden("ReCaptcha is not configured.")));
			}
			match recaptcha_verify::verify(
				self.services
					.config
					.recaptcha_private_site_key
					.as_ref()
					.unwrap(),
				r.response.as_str(),
				None,
			)
			.await
			{
				| Ok(()) => {
					uiaainfo.completed.push(AuthType::ReCaptcha);
				},
				| Err(e) => {
					error!("ReCaptcha verification failed: {e:?}");
					uiaainfo.auth_error = Some(StandardErrorBody {
						kind: ErrorKind::forbidden(),
						message: "ReCaptcha verification failed.".to_owned(),
					});
					return Ok((false, uiaainfo));
				},
			}
		},
		| AuthData::RegistrationToken(t) => {
			let token = t.token.trim().to_owned();

			if let Some(valid_token) = self
				.services
				.registration_tokens
				.validate_token(token)
				.await
			{
				self.services
					.registration_tokens
					.mark_token_as_used(valid_token);

				uiaainfo.completed.push(AuthType::RegistrationToken);
			} else {
				uiaainfo.auth_error = Some(StandardErrorBody {
					kind: ErrorKind::forbidden(),
					message: "Invalid registration token.".to_owned(),
				});
				return Ok((false, uiaainfo));
			}
		},
		| AuthData::Dummy(_) => {
			uiaainfo.completed.push(AuthType::Dummy);
		},
		| k => error!("type not supported: {:?}", k),
	}

	// Check if a flow now succeeds
	let mut completed = false;
	'flows: for flow in &mut uiaainfo.flows {
		for stage in &flow.stages {
			if !uiaainfo.completed.contains(stage) {
				continue 'flows;
			}
		}
		// We didn't break, so this flow succeeded!
		completed = true;
	}

	if !completed {
		self.update_uiaa_session(
			user_id,
			device_id,
			uiaainfo.session.as_ref().expect("session is always set"),
			Some(&uiaainfo),
		);

		return Ok((false, uiaainfo));
	}

	// UIAA was successful! Remove this session and return true
	self.update_uiaa_session(
		user_id,
		device_id,
		uiaainfo.session.as_ref().expect("session is always set"),
		None,
	);

	Ok((true, uiaainfo))
}

#[implement(Service)]
fn set_uiaa_request(
	&self,
	user_id: &UserId,
	device_id: &DeviceId,
	session: &str,
	request: &CanonicalJsonValue,
) {
	let key = (user_id.to_owned(), device_id.to_owned(), session.to_owned());
	self.userdevicesessionid_uiaarequest
		.write()
		.insert(key, request.to_owned());
}

#[implement(Service)]
pub fn get_uiaa_request(
	&self,
	user_id: &UserId,
	device_id: Option<&DeviceId>,
	session: &str,
) -> Option<CanonicalJsonValue> {
	let key = (
		user_id.to_owned(),
		device_id.unwrap_or_else(|| EMPTY.into()).to_owned(),
		session.to_owned(),
	);

	self.userdevicesessionid_uiaarequest
		.read()
		.get(&key)
		.cloned()
}

#[implement(Service)]
fn update_uiaa_session(
	&self,
	user_id: &UserId,
	device_id: &DeviceId,
	session: &str,
	uiaainfo: Option<&UiaaInfo>,
) {
	let key = (user_id, device_id, session);

	if let Some(uiaainfo) = uiaainfo {
		self.db
			.userdevicesessionid_uiaainfo
			.put(key, Json(uiaainfo));
	} else {
		self.db.userdevicesessionid_uiaainfo.del(key);
	}
}

#[implement(Service)]
async fn get_uiaa_session(
	&self,
	user_id: &UserId,
	device_id: &DeviceId,
	session: &str,
) -> Result<UiaaInfo> {
	let key = (user_id, device_id, session);
	self.db
		.userdevicesessionid_uiaainfo
		.qry(&key)
		.await
		.deserialized()
		.map_err(|_| err!(Request(Forbidden("UIAA session does not exist."))))
}
