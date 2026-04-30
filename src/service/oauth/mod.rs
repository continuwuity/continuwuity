use std::{
	collections::{BTreeSet, HashMap},
	sync::{Arc, Mutex},
	time::{Duration, SystemTime},
};

use base64::Engine;
use conduwuit::{
	Err, Result, err, info,
	utils::{self, hash::sha256},
};
use database::{Deserialized, Json, Map};
use itertools::Itertools;
use ruma::{DeviceId, OwnedDeviceId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
	Dep, config,
	oauth::{
		client_metadata::{ApplicationType, ClientMetadata, ResponseType},
		grant::{
			AuthorizationCodeQuery, AuthorizationCodeResponse, CodeChallengeMethod, ResponseMode,
			Scope, TokenRequest, TokenResponse, TokenType,
		},
	},
	users,
};

pub mod client_metadata;
pub mod grant;

pub struct Service {
	services: Services,
	db: Data,
	tickets: Mutex<HashMap<String, HashMap<OAuthTicket, SystemTime>>>,
	pending_code_grants: tokio::sync::Mutex<HashMap<String, PendingCodeGrant>>,
}

struct Data {
	clientid_clientmetadata: Arc<Map>,
	userdeviceid_oauthsessioninfo: Arc<Map>,
	refreshtoken_refreshtokeninfo: Arc<Map>,
}

struct Services {
	config: Dep<config::Service>,
	users: Dep<users::Service>,
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

struct PendingCodeGrant {
	authorizing_user: OwnedUserId,
	requested_scopes: BTreeSet<Scope>,
	client_name: Option<String>,
	expected_client_id: String,
	expected_redirect_uri: Url,
	code_challenge: String,
	requested_at: SystemTime,
}

impl PendingCodeGrant {
	const MAX_AGE: Duration = Duration::from_mins(1);
	const RANDOM_CODE_LENGTH: usize = 32;

	#[must_use]
	pub(crate) fn generate_code() -> String { utils::random_string(Self::RANDOM_CODE_LENGTH) }

	#[must_use]
	pub(crate) fn is_valid_for(&self, client_id: &str) -> bool {
		let now = SystemTime::now();

		self.expected_client_id == client_id
			&& now
				.duration_since(self.requested_at)
				.is_ok_and(|age| age < Self::MAX_AGE)
	}
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
				config: args.depend::<config::Service>("config"),
				users: args.depend::<users::Service>("users"),
			},
			db: Data {
				clientid_clientmetadata: args.db["clientid_clientmetadata"].clone(),
				userdeviceid_oauthsessioninfo: args.db["userdeviceid_oauthsessioninfo"].clone(),
				refreshtoken_refreshtokeninfo: args.db["refreshtoken_refreshtokeninfo"].clone(),
			},
			tickets: Mutex::default(),
			pending_code_grants: tokio::sync::Mutex::default(),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	const ACCESS_TOKEN_MAX_AGE: Duration = Duration::from_hours(1);
	const RANDOM_TOKEN_LENGTH: usize = 32;

	fn generate_token() -> String { utils::random_string(Self::RANDOM_TOKEN_LENGTH) }

	pub async fn register_client(
		&self,
		metadata: &ClientMetadata,
	) -> Result<String, &'static str> {
		metadata.validate()?;

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

		let code = PendingCodeGrant::generate_code();

		info!(
			client_id = &query.client_id,
			client_name = &client_metadata.client_name,
			?requested_scopes,
			?authorizing_user,
			"Issuing oauth authorization code"
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

		let pending_grant = PendingCodeGrant {
			authorizing_user,
			requested_scopes,
			client_name: client_metadata.client_name,
			expected_client_id: query.client_id,
			expected_redirect_uri: query.redirect_uri,
			code_challenge: query.code_challenge,
			requested_at: SystemTime::now(),
		};

		self.pending_code_grants
			.lock()
			.await
			.insert(code, pending_grant);

		Ok(redirect_uri)
	}

	pub async fn issue_token(&self, request: TokenRequest) -> Result<TokenResponse> {
		match request {
			| TokenRequest::AuthorizationCode {
				code,
				redirect_uri,
				client_id,
				code_verifier,
			} => {
				let mut pending_grants = self.pending_code_grants.lock().await;

				let Some(pending_grant) = pending_grants
					.remove(&code)
					.filter(|grant| grant.is_valid_for(&client_id))
				else {
					return Err!("Invalid code");
				};

				if redirect_uri != pending_grant.expected_redirect_uri {
					return Err!("Unexpected redirect uri");
				}

				let expected_code_challenge =
					base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(sha256::hash(&code_verifier));
				if expected_code_challenge != pending_grant.code_challenge {
					return Err!("Invalid code challenge");
				}

				self.create_session(
					pending_grant.authorizing_user,
					pending_grant.requested_scopes,
					pending_grant.client_name,
					client_id,
				)
				.await
			},
			| TokenRequest::RefreshToken { client_id, refresh_token } =>
				self.refresh_session(client_id, refresh_token).await,
		}
	}

	pub async fn revoke_token(&self, token: String) -> Result<()> {
		let (user_id, device_id) = if let Ok(refresh_token_info) = self
			.db
			.refreshtoken_refreshtokeninfo
			.get(&token)
			.await
			.deserialized::<RefreshTokenInfo>()
		{
			(refresh_token_info.user_id, refresh_token_info.device_id)
		} else if let Some(user) = self.services.users.find_from_token(&token).await {
			user
		} else {
			return Err!("Invalid token");
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
	) -> Result<TokenResponse> {
		let access_token = Self::generate_token();
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
			.ok_or_else(|| err!("No device ID scope supplied"))?;

		self.services
			.users
			.create_device(
				&authorizing_user,
				device_id,
				&access_token,
				Some(Self::ACCESS_TOKEN_MAX_AGE),
				client_name,
				None,
			)
			.await?;

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
			access_token,
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
	) -> Result<TokenResponse> {
		let Some(refresh_token_info) = self
			.db
			.refreshtoken_refreshtokeninfo
			.get(&refresh_token)
			.await
			.deserialized::<RefreshTokenInfo>()
			.ok()
		else {
			return Err!("Invalid refresh token");
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

		let new_access_token = Self::generate_token();
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
				&new_access_token,
				Some(Self::ACCESS_TOKEN_MAX_AGE),
			)
			.await?;

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
			access_token: new_access_token,
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
