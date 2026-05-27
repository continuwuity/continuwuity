use std::{
	borrow::Cow,
	collections::BTreeSet,
	error::Error,
	fmt::{Debug, Display},
	hash::Hash,
	mem::discriminant,
};

use regex::Regex;
use ruma::{OwnedDeviceId, api::OAuthScope};
use serde::{Deserialize, Serialize};
use url::Url;

use super::client_metadata::ResponseType;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthorizationCodeQuery {
	pub response_type: ResponseType,
	pub client_id: String,
	pub redirect_uri: Url,
	pub scope: RawScopes,
	pub state: String,
	#[serde(default)]
	pub response_mode: ResponseMode,
	pub code_challenge: String,
	pub code_challenge_method: CodeChallengeMethod,
	#[serde(default)]
	pub prompt: Option<Prompt>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResponseMode {
	#[default]
	// default for `code` response type, see https://openid.net/specs/oauth-v2-multiple-response-types-1_0.html#:~:text=Client%2E-,For,encoding%2E,-See
	Query,
	Fragment,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[non_exhaustive]
pub enum CodeChallengeMethod {
	S256,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Prompt {
	Create,
	#[serde(other)]
	Unknown,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialOrd, Ord)]
pub enum RequestedScope {
	Device(OwnedDeviceId),
	FullAccess,
	ServerAdministration,
}

impl RequestedScope {
	pub fn as_granted_scope(&self) -> Option<OAuthScope> {
		match self {
			| Self::FullAccess => Some(OAuthScope::FullAccess),
			| Self::ServerAdministration => Some(OAuthScope::ServerAdministration),
			| Self::Device(_) => None,
		}
	}
}

impl PartialEq for RequestedScope {
	fn eq(&self, other: &Self) -> bool { discriminant(self) == discriminant(other) }
}

impl Eq for RequestedScope {}

impl Hash for RequestedScope {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) { discriminant(self).hash(state); }
}

impl Display for RequestedScope {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let urn = match self {
			| Self::FullAccess => "urn:matrix:client:api:*".to_owned(),
			| Self::Device(device_id) => format!("urn:matrix:client:device:{device_id}"),
			| Self::ServerAdministration =>
				"urn:matrix:client:cc.c10y.msc4484.server_administration".to_owned(),
		};

		f.write_str(&urn)
	}
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawScopes(String);

impl RawScopes {
	pub fn to_scopes(&self) -> Result<BTreeSet<RequestedScope>, String> {
		let full_access_regex =
			Regex::new(r"urn:matrix:(client|org.matrix.msc2967.client):api:\*").unwrap();
		let device_token_regex = Regex::new(
			r"urn:matrix:(client|org.matrix.msc2967.client):device:([a-zA-Z0-9-._~]{5,})",
		)
		.unwrap();
		let server_administration_regex =
			Regex::new(r"urn:matrix:client:cc.c10y.msc4484.server_administration").unwrap();

		let mut scopes = BTreeSet::new();

		for token in self.0.split(' ') {
			let scope_was_new = {
				if full_access_regex.is_match(token) {
					scopes.insert(RequestedScope::FullAccess)
				} else if let Some(captures) = device_token_regex.captures(token) {
					scopes
						.insert(RequestedScope::Device(captures.get(2).unwrap().as_str().into()))
				} else if server_administration_regex.is_match(token) {
					scopes.insert(RequestedScope::ServerAdministration)
				} else if token == "openid" {
					// TODO(unspecced): Element sets this scope but doesn't use it for anything
					true
				} else {
					return Err(format!("Invalid scope: {token}"));
				}
			};

			if !scope_was_new {
				return Err("Scope was specified more than once".to_owned());
			}
		}

		Ok(scopes)
	}
}

#[derive(Serialize, Debug, Clone)]
pub struct OAuthError {
	pub error: ErrorCode,
	pub error_description: Cow<'static, str>,
}

impl OAuthError {
	pub const fn invalid_request(error_description: &'static str) -> Self {
		Self {
			error: ErrorCode::InvalidRequest,
			error_description: Cow::Borrowed(error_description),
		}
	}

	pub const fn invalid_grant(error_description: &'static str) -> Self {
		Self {
			error: ErrorCode::InvalidGrant,
			error_description: Cow::Borrowed(error_description),
		}
	}
}

impl Display for OAuthError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "OAuth error {:?}: {}", self.error, self.error_description)
	}
}

impl Error for OAuthError {}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
	InvalidRequest,
	AccessDenied,
	InvalidScope,
	InvalidGrant,
	InvalidClientMetadata,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum AuthorizationCodeResponse {
	Success {
		state: String,
		code: String,
	},
	Error(OAuthError),
}

#[derive(Deserialize)]
#[serde(tag = "grant_type", rename_all = "snake_case")]
pub enum TokenRequest {
	AuthorizationCode {
		code: String,
		redirect_uri: Url,
		client_id: String,
		code_verifier: String,
	},
	RefreshToken {
		client_id: String,
		refresh_token: String,
	},
}

impl TokenRequest {
	#[must_use]
	pub fn client_id(&self) -> &str {
		match self {
			| Self::AuthorizationCode { client_id, .. }
			| Self::RefreshToken { client_id, .. } => client_id,
		}
	}
}

#[derive(Serialize)]
pub struct TokenResponse {
	pub access_token: String,
	pub token_type: TokenType,
	pub expires_in: u64,
	pub refresh_token: String,
	pub scope: String,
}

#[derive(Serialize)]
pub enum TokenType {
	Bearer,
}

#[derive(Deserialize)]
pub struct RevokeTokenRequest {
	pub token: String,
}