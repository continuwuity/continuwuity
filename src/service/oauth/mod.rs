use std::{
	collections::{BTreeSet, HashMap},
	sync::{Arc, Mutex},
	time::{Duration, SystemTime},
};

use base64::Engine;
use conduwuit::{
	Result, info,
	utils::{self, hash::sha256},
};
use database::{Deserialized, Json, Map};
use itertools::Itertools;
use lru_cache::LruCache;
use rand::distr::{Distribution, slice::Choose};
use ruma::{DeviceId, OwnedDeviceId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
	Dep, config,
	oauth::{
		client_metadata::{ApplicationType, ClientMetadata, ResponseType},
		grant::{
			AuthorizationCodeQuery, AuthorizationCodeResponse, CodeChallengeMethod,
			DeviceCodeRequest, DeviceCodeResponse, ErrorCode, OAuthError, ResponseMode, Scope,
			TokenRequest, TokenRequestType, TokenResponse, TokenType,
		},
	},
	users::{self, DeviceToken},
};

pub mod client_metadata;
pub mod grant;

pub struct Service {
	services: Services,
	db: Data,
	tickets: Mutex<HashMap<String, HashMap<OAuthTicket, SystemTime>>>,
	pending_auth_code_grants: tokio::sync::Mutex<LruCache<String, PendingAuthCodeGrant>>,
	pending_device_code_grants: tokio::sync::Mutex<LruCache<String, PendingDeviceCodeGrant>>,
}

struct Data {
	clientid_clientmetadata: Arc<Map>,
	userdeviceid_oauthsessioninfo: Arc<Map>,
	refreshtoken_refreshtokeninfo: Arc<Map>,
}

struct Services {
	users: Dep<users::Service>,
	config: Dep<config::Service>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SessionInfo {
	pub client_id: String,
	pub scopes: BTreeSet<Scope>,
	current_refresh_token: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct RefreshTokenInfo {
	client_id: String,
	user_id: OwnedUserId,
	device_id: OwnedDeviceId,
}

struct PendingAuthCodeGrant {
	authorizing_user: OwnedUserId,
	requested_scopes: BTreeSet<Scope>,
	client_name: Option<String>,
	expected_client_id: String,
	expected_redirect_uri: Url,
	code_challenge: String,
	requested_at: SystemTime,
}

impl PendingAuthCodeGrant {
	const MAX_AGE: Duration = Duration::from_mins(1);

	#[must_use]
	pub(crate) fn is_valid_for(&self, client_id: &str) -> bool {
		let now = SystemTime::now();

		self.expected_client_id == client_id
			&& now
				.duration_since(self.requested_at)
				.is_ok_and(|age| age < Self::MAX_AGE)
	}
}

struct PendingDeviceCodeGrant {
	state: DeviceCodeGrantState,
	requested_scopes: BTreeSet<Scope>,
	client_name: Option<String>,
	client_id: String,
	requested_at: SystemTime,
}

enum DeviceCodeGrantState {
	Unverified {
		user_code: String,
	},
	Verified {
		authorizing_user: OwnedUserId,
	},
}

impl PendingDeviceCodeGrant {
	const MAX_AGE: Duration = Duration::from_mins(1);

	#[must_use]
	pub(crate) fn is_valid_for(&self, client_id: &str) -> bool {
		let now = SystemTime::now();

		self.client_id == client_id
			&& now
				.duration_since(self.requested_at)
				.is_ok_and(|age| age < Self::MAX_AGE)
	}
}

pub struct DeviceCodeGrantInfo {
	pub device_code: String,
	pub client_metadata: ClientMetadata,
	pub requested_scopes: BTreeSet<Scope>,
}

/// A time-limited grant for a client to perform some sensitive action.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OAuthTicket {
	CrossSigningReset,
}

impl OAuthTicket {
	const MAX_AGE: Duration = Duration::from_mins(10);

	#[must_use]
	pub fn ticket_issue_path(&self) -> &'static str {
		match self {
			| Self::CrossSigningReset => "/account/cross_signing_reset",
		}
	}
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				users: args.depend::<users::Service>("users"),
				config: args.depend::<config::Service>("config"),
			},
			db: Data {
				clientid_clientmetadata: args.db["clientid_clientmetadata"].clone(),
				userdeviceid_oauthsessioninfo: args.db["userdeviceid_oauthsessioninfo"].clone(),
				refreshtoken_refreshtokeninfo: args.db["refreshtoken_refreshtokeninfo"].clone(),
			},
			tickets: Mutex::default(),
			pending_auth_code_grants: tokio::sync::Mutex::new(LruCache::new(
				Self::MAX_PENDING_GRANTS,
			)),
			pending_device_code_grants: tokio::sync::Mutex::new(LruCache::new(
				Self::MAX_PENDING_GRANTS,
			)),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	const ACCESS_TOKEN_MAX_AGE: Duration = Duration::from_hours(1);
	// Maximum number of pending code grants which will be held in memory at once,
	// to prevent unbounded memory use if someone decides to repeatedly reload the
	// grant page.
	const MAX_PENDING_GRANTS: usize = 100;
	const RANDOM_TOKEN_LENGTH: usize = 32;
	const USER_CODE_CHARACTERS: &[char] = &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];
	const USER_CODE_LENGTH: usize = 6;

	fn generate_token() -> String { utils::random_string(Self::RANDOM_TOKEN_LENGTH) }

	fn generate_user_code() -> String {
		Choose::new(Self::USER_CODE_CHARACTERS)
			.unwrap()
			.sample_iter(&mut rand::rng())
			.take(Self::USER_CODE_LENGTH)
			.collect()
	}

	pub async fn register_client(&self, metadata: &ClientMetadata) -> Result<String, OAuthError> {
		metadata.validate().map_err(|error| OAuthError {
			error: ErrorCode::InvalidClientMetadata,
			error_description: error.into(),
		})?;

		let client_id = base64::prelude::BASE64_STANDARD
			.encode(sha256::hash(serde_json::to_string(metadata).unwrap().as_bytes()));

		if self
			.db
			.clientid_clientmetadata
			.exists(&client_id)
			.await
			.is_err()
		{
			self.db
				.clientid_clientmetadata
				.raw_put(&client_id, Json(metadata.clone()));
		}

		Ok(client_id)
	}

	pub async fn get_client_metadata(&self, client_id: &str) -> Option<ClientMetadata> {
		self.db
			.clientid_clientmetadata
			.get(client_id)
			.await
			.deserialized()
			.ok()
	}

	pub async fn get_session_info_for_device(
		&self,
		user_id: &UserId,
		device_id: &DeviceId,
	) -> Option<SessionInfo> {
		self.db
			.userdeviceid_oauthsessioninfo
			.qry(&(user_id, device_id))
			.await
			.deserialized::<SessionInfo>()
			.ok()
	}

	pub async fn request_authorization_code(
		&self,
		authorizing_user: OwnedUserId,
		query: AuthorizationCodeQuery,
	) -> Result<String, String> {
		let Some(client_metadata) = self.get_client_metadata(&query.client_id).await else {
			return Err("Invalid client ID".to_owned());
		};

		if !(client_metadata
			.response_types
			.contains(&query.response_type)
			&& matches!(query.response_type, ResponseType::Code))
		{
			return Err("Invalid response type".to_owned());
		}

		if !matches!(query.code_challenge_method, CodeChallengeMethod::S256) {
			return Err("Invalid code challenge type".to_owned());
		}

		{
			let mut stripped_uri = query.redirect_uri.clone();

			if client_metadata.application_type == ApplicationType::Native
				&& query
					.redirect_uri
					.host_str()
					.is_some_and(|host| ClientMetadata::ACCEPTABLE_LOCALHOSTS.contains(&host))
			{
				// Remove the port from localhost redirect URIs for native applications when
				// checking if it's valid
				stripped_uri.set_port(None).unwrap();
			}

			if !client_metadata.redirect_uris.contains(&stripped_uri) {
				return Err("Invalid redirect URI".to_owned());
			}
		}

		let requested_scopes = query.scope.to_scopes()?;

		let redirect_uri_query_separator = match query.response_mode {
			| ResponseMode::Fragment => '#',
			| ResponseMode::Query => '?',
		};

		let code = Self::generate_token();

		info!(
			client_id = &query.client_id,
			client_name = &client_metadata.client_name,
			?requested_scopes,
			?authorizing_user,
			"Issuing OAuth authorization code"
		);

		let redirect_uri = format!(
			"{}{}{}",
			query.redirect_uri,
			redirect_uri_query_separator,
			serde_urlencoded::to_string(AuthorizationCodeResponse {
				state: query.state,
				code: code.clone(),
			})
			.unwrap(),
		);

		let pending_grant = PendingAuthCodeGrant {
			authorizing_user,
			requested_scopes,
			client_name: client_metadata.client_name,
			expected_client_id: query.client_id,
			expected_redirect_uri: query.redirect_uri,
			code_challenge: query.code_challenge,
			requested_at: SystemTime::now(),
		};

		self.pending_auth_code_grants
			.lock()
			.await
			.insert(code, pending_grant);

		Ok(redirect_uri)
	}

	pub async fn request_device_code(
		&self,
		query: DeviceCodeRequest,
	) -> Result<DeviceCodeResponse, OAuthError> {
		let Some(client_metadata) = self.get_client_metadata(&query.client_id).await else {
			return Err(OAuthError::invalid_grant("Invalid client ID"));
		};

		let requested_scopes = query
			.scope
			.to_scopes()
			.map_err(|err| OAuthError::new(ErrorCode::InvalidGrant, err))?;

		let device_code = Self::generate_token();
		let user_code = Self::generate_user_code();

		let verification_uri = self
			.services
			.config
			.get_client_domain()
			.join(&format!("{}/oauth2/grant/device_code", conduwuit::ROUTE_PREFIX))
			.unwrap();

		let mut verification_uri_complete = verification_uri.clone();
		verification_uri_complete
			.query_pairs_mut()
			.append_pair("user_code", &user_code);

		info!(
			client_id = &query.client_id,
			client_name = &client_metadata.client_name,
			?requested_scopes,
			"Issuing OAuth device code"
		);

		let pending_grant = PendingDeviceCodeGrant {
			state: DeviceCodeGrantState::Unverified { user_code: user_code.clone() },
			requested_scopes,
			client_name: client_metadata.client_name,
			client_id: query.client_id,
			requested_at: SystemTime::now(),
		};

		self.pending_device_code_grants
			.lock()
			.await
			.insert(device_code.clone(), pending_grant);

		Ok(DeviceCodeResponse {
			device_code,
			user_code,
			verification_uri,
			verification_uri_complete: Some(verification_uri_complete),
			expires_in: PendingDeviceCodeGrant::MAX_AGE.as_secs(),
		})
	}

	pub async fn grant_info_for_user_code(
		&self,
		supplied_user_code: &str,
	) -> Option<DeviceCodeGrantInfo> {
		let pending_grants = self.pending_device_code_grants.lock().await;

		let (device_code, grant) = pending_grants
			.iter()
			.find(|(_, grant)| {
				matches!(&grant.state, DeviceCodeGrantState::Unverified { user_code } if user_code == supplied_user_code)
			})?;

		let client_metadata = self
			.get_client_metadata(&grant.client_id)
			.await
			.expect("client should exist");

		Some(DeviceCodeGrantInfo {
			device_code: device_code.clone(),
			client_metadata,
			requested_scopes: grant.requested_scopes.clone(),
		})
	}

	pub async fn validate_device_code(
		&self,
		authorizing_user: OwnedUserId,
		device_code: &str,
	) -> Result<(), String> {
		let mut pending_grants = self.pending_device_code_grants.lock().await;

		let Some(pending_grant) = pending_grants.get_mut(device_code) else {
			return Err("Invalid device code".to_owned());
		};

		match &mut pending_grant.state {
			| state @ DeviceCodeGrantState::Unverified { .. } => {
				*state = DeviceCodeGrantState::Verified { authorizing_user };

				Ok(())
			},
			| DeviceCodeGrantState::Verified {
				authorizing_user: previous_authorizing_user,
			} =>
				if *previous_authorizing_user == authorizing_user {
					Ok(())
				} else {
					Err("Device code is already verified".to_owned())
				},
		}
	}

	pub async fn issue_token(&self, request: TokenRequest) -> Result<TokenResponse, OAuthError> {
		let TokenRequest { client_id, request } = request;

		let Some(client_metadata) = self.get_client_metadata(&client_id).await else {
			return Err(OAuthError::invalid_request("Invalid client ID"));
		};

		if !client_metadata.grant_types.contains(&request.grant_type()) {
			return Err(OAuthError::invalid_grant("Client cannot request this grant type"));
		}

		match request {
			| TokenRequestType::AuthorizationCode { code, redirect_uri, code_verifier } => {
				let mut pending_grants = self.pending_auth_code_grants.lock().await;

				let Some(pending_grant) = pending_grants
					.remove(&code)
					.filter(|grant| grant.is_valid_for(&client_id))
				else {
					return Err(OAuthError::invalid_grant("Invalid authorization code"));
				};

				if redirect_uri != pending_grant.expected_redirect_uri {
					return Err(OAuthError::invalid_grant("Invalid redirect URI"));
				}

				let expected_code_challenge =
					base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(sha256::hash(&code_verifier));
				if expected_code_challenge != pending_grant.code_challenge {
					return Err(OAuthError::invalid_grant("Invalid code challenge"));
				}

				self.create_session(
					pending_grant.authorizing_user,
					pending_grant.requested_scopes,
					pending_grant.client_name,
					client_id,
				)
				.await
			},
			| TokenRequestType::DeviceCode { device_code } => {
				let mut pending_grants = self.pending_device_code_grants.lock().await;

				let Some(pending_grant) = pending_grants
					.remove(&device_code)
					.filter(|grant| grant.is_valid_for(&client_id))
				else {
					return Err(OAuthError::new_static(
						ErrorCode::ExpiredToken,
						"Invalid device code",
					));
				};

				match &pending_grant.state {
					| DeviceCodeGrantState::Unverified { .. } => {
						pending_grants.insert(device_code, pending_grant);

						Err(OAuthError::new_static(
							ErrorCode::AuthorizationPending,
							"Authorization is pending",
						))
					},
					| DeviceCodeGrantState::Verified { authorizing_user } =>
						self.create_session(
							authorizing_user.to_owned(),
							pending_grant.requested_scopes,
							pending_grant.client_name,
							client_id,
						)
						.await,
				}
			},
			| TokenRequestType::RefreshToken { refresh_token } =>
				self.refresh_session(client_id, refresh_token).await,
		}
	}

	pub async fn revoke_token(&self, token: String) -> Result<(), OAuthError> {
		let (user_id, device_id) = if let Ok(refresh_token_info) = self
			.db
			.refreshtoken_refreshtokeninfo
			.get(&token)
			.await
			.deserialized::<RefreshTokenInfo>()
		{
			(refresh_token_info.user_id, refresh_token_info.device_id)
		} else if let Some((user_id, device_id, _)) =
			self.services.users.find_from_token(&token).await
		{
			(user_id, device_id)
		} else {
			return Err(OAuthError::invalid_grant("Invalid access or refersh token"));
		};

		// This will also call [`Self::remove_session`]
		self.services
			.users
			.remove_device(&user_id, &device_id)
			.await;

		Ok(())
	}

	async fn create_session(
		&self,
		authorizing_user: OwnedUserId,
		requested_scopes: BTreeSet<Scope>,
		client_name: Option<String>,
		client_id: String,
	) -> Result<TokenResponse, OAuthError> {
		let access_token = DeviceToken::new_random().with_max_age(Self::ACCESS_TOKEN_MAX_AGE);
		let refresh_token = Self::generate_token();

		let device_id = requested_scopes
			.iter()
			.find_map(|scope| {
				if let Scope::Device(device_id) = scope {
					Some(device_id)
				} else {
					None
				}
			})
			.ok_or_else(|| OAuthError::invalid_grant("No device ID scope supplied"))?;

		if self
			.services
			.users
			.get_device_metadata(&authorizing_user, device_id)
			.await
			.is_ok()
		{
			return Err(OAuthError::new_static(
				ErrorCode::InvalidScope,
				"A device with the supplied ID already exists for this user",
			));
		}

		self.services
			.users
			.create_device(
				&authorizing_user,
				device_id,
				Some(access_token.clone()),
				client_name,
				None,
			)
			.await
			// This can only panic if the authorizing user suffered a spontaneous existence
			// failure during authentication, which should(?) be impossible(?)
			.expect("failed to create device");

		self.db.userdeviceid_oauthsessioninfo.put(
			(&authorizing_user, device_id),
			Json(SessionInfo {
				client_id: client_id.clone(),
				current_refresh_token: refresh_token.clone(),
				scopes: requested_scopes.clone(),
			}),
		);

		self.db.refreshtoken_refreshtokeninfo.raw_put(
			&refresh_token,
			Json(RefreshTokenInfo {
				client_id: client_id.clone(),
				user_id: authorizing_user.clone(),
				device_id: device_id.to_owned(),
			}),
		);

		info!(
			?client_id,
			?authorizing_user,
			?device_id,
			?requested_scopes,
			"Created new oauth session"
		);

		Ok(TokenResponse {
			access_token: access_token.into_token(),
			token_type: TokenType::Bearer,
			expires_in: Self::ACCESS_TOKEN_MAX_AGE.as_secs(),
			scope: requested_scopes.iter().join(" "),
			refresh_token,
		})
	}

	async fn refresh_session(
		&self,
		client_id: String,
		refresh_token: String,
	) -> Result<TokenResponse, OAuthError> {
		let Some(refresh_token_info) = self
			.db
			.refreshtoken_refreshtokeninfo
			.get(&refresh_token)
			.await
			.deserialized::<RefreshTokenInfo>()
			.ok()
		else {
			return Err(OAuthError::invalid_grant("Invalid refresh token"));
		};

		assert_eq!(&client_id, &refresh_token_info.client_id, "refresh token client id mismatch");

		let mut session_info = self
			.get_session_info_for_device(
				&refresh_token_info.user_id,
				&refresh_token_info.device_id,
			)
			.await
			.expect("session info should exist");

		assert_eq!(&client_id, &session_info.client_id, "session info client id mismatch");

		let new_access_token = DeviceToken::new_random().with_max_age(Self::ACCESS_TOKEN_MAX_AGE);
		let new_refresh_token = Self::generate_token();
		let scope = session_info.scopes.iter().join(" ");
		session_info
			.current_refresh_token
			.clone_from(&new_refresh_token);

		self.services
			.users
			.set_token(
				&refresh_token_info.user_id,
				&refresh_token_info.device_id,
				new_access_token.clone(),
			)
			.await
			.expect("should be able to set token");

		self.db.userdeviceid_oauthsessioninfo.put(
			(&refresh_token_info.user_id, &refresh_token_info.device_id),
			Json(session_info),
		);

		self.db.refreshtoken_refreshtokeninfo.remove(&refresh_token);
		drop(refresh_token);
		self.db
			.refreshtoken_refreshtokeninfo
			.raw_put(&new_refresh_token, Json(refresh_token_info));

		Ok(TokenResponse {
			access_token: new_access_token.into_token(),
			token_type: TokenType::Bearer,
			expires_in: Self::ACCESS_TOKEN_MAX_AGE.as_secs(),
			scope,
			refresh_token: new_refresh_token,
		})
	}

	pub async fn remove_session(&self, user_id: &UserId, device_id: &DeviceId) {
		let session_info = self.get_session_info_for_device(user_id, device_id).await;

		if let Some(session_info) = session_info {
			self.db
				.refreshtoken_refreshtokeninfo
				.remove(&session_info.current_refresh_token);
			self.db
				.userdeviceid_oauthsessioninfo
				.del((user_id, device_id));
			info!(?user_id, ?device_id, "Removed OAuth session");
		}
	}

	/// Issue a ticket for `localpart` to perform some action.
	pub fn issue_ticket(&self, localpart: String, ticket: OAuthTicket) {
		self.tickets
			.lock()
			.unwrap()
			.entry(localpart)
			.or_default()
			.insert(ticket, SystemTime::now());
	}

	/// Try to consume an unexpired ticket for `localpart`.
	pub fn try_consume_ticket(&self, localpart: &str, ticket: OAuthTicket) -> bool {
		let now = SystemTime::now();

		self.tickets
			.lock()
			.unwrap()
			.get_mut(localpart)
			.and_then(|tickets| tickets.remove(&ticket))
			.is_some_and(|issued| {
				now.duration_since(issued)
					.is_ok_and(|duration| duration < OAuthTicket::MAX_AGE)
			})
	}
}
