use std::{collections::BTreeSet, fmt::Debug, hash::Hash, mem::discriminant};

use regex::Regex;
use ruma::OwnedDeviceId;
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialOrd, Ord)]
pub enum Scope {
	Device(OwnedDeviceId),
	ClientApi,
}

impl PartialEq for Scope {
	fn eq(&self, other: &Self) -> bool { discriminant(self) == discriminant(other) }
}

impl Eq for Scope {}

impl Hash for Scope {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) { discriminant(self).hash(state); }
}

impl std::fmt::Display for Scope {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let urn = match self {
			| Self::ClientApi => "urn:matrix:client:api:*".to_owned(),
			| Self::Device(device_id) => format!("urn:matrix:client:device:{device_id}"),
		};

		f.write_str(&urn)
	}
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawScopes(String);

impl RawScopes {
	pub fn to_scopes(&self) -> Result<BTreeSet<Scope>, String> {
		let client_api_token_regex =
			Regex::new(r"urn:matrix:(client|org.matrix.msc2967.client):api:\*").unwrap();
		let device_token_regex = Regex::new(
			r"urn:matrix:(client|org.matrix.msc2967.client):device:([a-zA-Z0-9-._~]{5,})",
		)
		.unwrap();

		let mut scopes = BTreeSet::new();

		for token in self.0.split(' ') {
			let scope_was_new = {
				if client_api_token_regex.is_match(token) {
					scopes.insert(Scope::ClientApi)
				} else if let Some(captures) = device_token_regex.captures(token) {
					scopes.insert(Scope::Device(captures.get(2).unwrap().as_str().into()))
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

#[derive(Serialize)]
pub struct AuthorizationCodeResponse {
	pub state: String,
	pub code: String,
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
