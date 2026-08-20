//! VAPID key management and authorization-header generation for Web Push.

use std::{collections::HashMap, sync::Arc};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use conduwuit_core::{Result, SyncRwLock, debug_info, err, utils};
use conduwuit_database::Database;
use p256::{
	SecretKey,
	ecdsa::{Signature, SigningKey, signature::Signer},
	elliptic_curve::sec1::ToEncodedPoint,
};
use serde_json::json;

const VAPID_TTL_SECS: u64 = 12 * 60 * 60;
const VAPID_REFRESH_MARGIN_SECS: u64 = 60;

/// The server's VAPID keypair and cached authorization headers.
pub(crate) struct Vapid {
	secret: SecretKey,
	public_key: String,
	authorizations: SyncRwLock<HashMap<(String, String), (String, u64)>>,
}

impl Vapid {
	/// Loads the persistent VAPID keypair, generating one on first use.
	pub(crate) fn init(db: &Arc<Database>) -> Result<Self> {
		let secret = match db["global"].get_blocking(b"vapid_secret_key") {
			| Ok(ref val) => SecretKey::from_slice(val).map_err(|e| {
				err!(Database(error!(
					"Stored VAPID key is invalid: {e}. Refusing to replace it, as that would \
					 invalidate every existing web push subscription."
				)))
			})?,
			| Err(e) => {
				assert!(e.is_not_found(), "unexpected error fetching VAPID key");
				let secret = loop {
					if let Ok(secret) = SecretKey::from_slice(&rand::random::<[u8; 32]>()) {
						break secret;
					}
				};

				db["global"].raw_put(b"vapid_secret_key", &*secret.to_bytes());
				debug_info!("Generated new VAPID keypair for web push");
				secret
			},
		};

		let public_key =
			URL_SAFE_NO_PAD.encode(secret.public_key().to_encoded_point(false).as_bytes());

		Ok(Self {
			secret,
			public_key,
			authorizations: SyncRwLock::new(HashMap::new()),
		})
	}

	pub(super) fn public_key(&self) -> &str { &self.public_key }

	/// Returns a cached or freshly signed VAPID authorization for an endpoint.
	pub(super) fn authorization(&self, endpoint: &url::Url, contact: &str) -> Result<String> {
		let key = (endpoint.origin().ascii_serialization(), contact.to_owned());
		let now = utils::millis_since_unix_epoch().saturating_div(1000);

		if let Some((header, expires_at)) = self.authorizations.read().get(&key)
			&& now.saturating_add(VAPID_REFRESH_MARGIN_SECS) < *expires_at
		{
			return Ok(header.clone());
		}

		let expires_at = now.saturating_add(VAPID_TTL_SECS);
		let header = self.sign(&key.0, contact, expires_at)?;
		let mut authorizations = self.authorizations.write();
		authorizations.retain(|_, (_, expires_at)| now < *expires_at);
		authorizations.insert(key, (header.clone(), expires_at));

		Ok(header)
	}

	fn sign(&self, audience: &str, contact: &str, expires_at: u64) -> Result<String> {
		let header =
			URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({ "typ": "JWT", "alg": "ES256" }))?);
		let claims = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({
			"aud": audience,
			"exp": expires_at,
			"sub": contact,
		}))?);
		let signing_input = format!("{header}.{claims}");
		let signature: Signature = SigningKey::from(&self.secret).sign(signing_input.as_bytes());
		let signature = URL_SAFE_NO_PAD.encode(signature.to_bytes());

		Ok(format!("vapid t={signing_input}.{signature},k={}", self.public_key))
	}
}
