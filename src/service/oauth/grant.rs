use std::{
	borrow::Cow,
	collections::BTreeSet,
	error::Error,
	fmt::{Debug, Display},
};

use regex::regex;
use ruma::{OwnedDeviceId, api::OAuthClientScope};
use serde::{Deserialize, Serialize};
use url::Url;

use super::client_metadata::ResponseType;
use crate::oauth::client_metadata::GrantType;

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

#[derive(Deserialize, Serialize)]
pub struct AuthorizationCodeResponse {
	pub state: String,
	pub code: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceCodeRequest {
	pub client_id: String,
	pub scope: RawScopes,
}

#[derive(Deserialize, Serialize)]
pub struct DeviceCodeResponse {
	pub device_code: String,
	pub user_code: String,
	pub verification_uri: Url,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub verification_uri_complete: Option<Url>,
	pub expires_in: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeviceCodeVerifyQuery {
	pub user_code: Option<String>,
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

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RequestedScopes {
	pub device_id: Option<OwnedDeviceId>,
	pub scopes: BTreeSet<OAuthClientScope>,
}

impl Display for RequestedScopes {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut scopes: Vec<_> = self
			.scopes
			.iter()
			.map(OAuthClientScope::to_string)
			.collect();

		if let Some(device_id) = &self.device_id {
			scopes.push(device_id.to_string());
		}

		f.write_str(&scopes.join(" "))
	}
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawScopes(String);

impl RawScopes {
	#[allow(clippy::trivial_regex)]
	pub fn to_scopes(&self) -> Result<RequestedScopes, String> {
		let client_api_token_regex =
			regex!(r"urn:matrix:(client|org.matrix.msc2967.client):api:\*");
		let device_token_regex =
			regex!(r"urn:matrix:(client|org.matrix.msc2967.client):device:([a-zA-Z0-9-._~]{5,})");
		let server_administration_regex =
			regex!(r"urn:matrix:client:cc.c10y.msc4484.server_administration");

		let mut scopes = RequestedScopes::default();

		for token in self.0.split(' ') {
			let scope_was_new = {
				if client_api_token_regex.is_match(token) {
					scopes.scopes.insert(OAuthClientScope::ApiFullAccess)
				} else if server_administration_regex.is_match(token) {
					scopes.scopes.insert(OAuthClientScope::ServerAdministration)
				} else if let Some(captures) = device_token_regex.captures(token) {
					scopes
						.device_id
						.replace(captures.get(2).unwrap().as_str().into())
						.is_none()
				} else {
					continue;
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
	#[must_use]
	pub fn new(error: ErrorCode, error_description: String) -> Self {
		Self {
			error,
			error_description: Cow::Owned(error_description),
		}
	}

	#[must_use]
	pub const fn new_static(error: ErrorCode, error_description: &'static str) -> Self {
		Self {
			error,
			error_description: Cow::Borrowed(error_description),
		}
	}

	#[must_use]
	pub const fn invalid_request(error_description: &'static str) -> Self {
		Self::new_static(ErrorCode::InvalidRequest, error_description)
	}

	#[must_use]
	pub const fn invalid_grant(error_description: &'static str) -> Self {
		Self::new_static(ErrorCode::InvalidGrant, error_description)
	}

	#[must_use]
	pub const fn unauthorized_client(error_description: &'static str) -> Self {
		Self::new_static(ErrorCode::UnauthorizedClient, error_description)
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
	InvalidClient,
	InvalidClientMetadata,
	UnauthorizedClient,
	AuthorizationPending,
	ExpiredToken,
}

#[derive(Deserialize)]
pub struct TokenRequest {
	pub client_id: String,
	#[serde(flatten)]
	pub request: TokenRequestType,
}

#[derive(Deserialize)]
#[serde(tag = "grant_type", rename_all = "snake_case")]
pub enum TokenRequestType {
	AuthorizationCode {
		code: String,
		redirect_uri: Url,
		code_verifier: String,
	},
	#[serde(rename = "urn:ietf:params:oauth:grant-type:device_code")]
	DeviceCode {
		device_code: String,
	},
	RefreshToken {
		refresh_token: String,
	},
}

impl TokenRequestType {
	#[must_use]
	pub fn grant_type(&self) -> GrantType {
		match self {
			| Self::AuthorizationCode { .. } => GrantType::AuthorizationCode,
			| Self::DeviceCode { .. } => GrantType::DeviceCode,
			| Self::RefreshToken { .. } => GrantType::RefreshToken,
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
