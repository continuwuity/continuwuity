use conduwuit::{
	Err, Result, matrix::event::gen_event_id_canonical_json, trace, utils::to_canonical_object,
};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, ServerName, SigningKeyAlgorithm,
	api::federation::discovery::ServerSigningKeys,
	room_version_rules::RoomVersionRules,
	serde::{Base64, base64::Standard},
	signatures::Verified,
};
use serde_json::value::RawValue as RawJsonValue;

impl super::Service {
	/// Validates the incoming event only using locally known verification keys.
	/// Inserts the calculated event ID into the event_id field upon success.
	pub async fn verify_event_json_no_fetch_add_event_id(
		&self,
		pdu: &RawJsonValue,
		room_version_rules: &RoomVersionRules,
	) -> Result<(OwnedEventId, CanonicalJsonObject)> {
		// TODO(nex): this function is used in two places (same file) and is
		// awfully named, can we just kill it?
		let (event_id, mut value) = gen_event_id_canonical_json(pdu, room_version_rules)?;
		if !self.required_keys_exist(&value, room_version_rules).await {
			return Err!(BadServerResponse(debug_warn!(
				"Event {event_id} cannot be verified: missing verify key(s) locally."
			)));
		}
		if let Err(e) = self.verify_event_json(&value, room_version_rules).await {
			return Err!(BadServerResponse(debug_error!(
				"Event {event_id} failed verification: {e:?}"
			)));
		}

		value.insert("event_id".into(), CanonicalJsonValue::String(event_id.as_str().into()));

		Ok((event_id, value))
	}

	/// Verifies the incoming event after fetching the keys required to do so.
	pub async fn verify_event_json(
		&self,
		event: &CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result<Verified> {
		let keys = self.get_event_keys(event, room_version_rules).await?;
		ruma::signatures::verify_event(&keys, event, room_version_rules).map_err(Into::into)
	}

	/// Verifies a server signing keys response, either as the
	/// result of `GET /_matrix/key/v2/server`, or a chunk from
	/// `POST /_matrix/key/v2/query`.
	///
	/// Returns an error if the self-signature is invalid, or strict notary
	/// validation is used (`notary` is non-empty) and the notary did not sign
	/// the response.
	///
	/// `notary` must either be empty or have ONE entry. Extraneous notaries
	/// will be ignored.
	pub fn verify_server_keys_response(
		server_keys: &ServerSigningKeys,
		notary: Option<(&ServerName, &[Base64])>,
	) -> Result<()> {
		if let Some((notary_name, notary_keys)) = notary {
			if !notary_keys.is_empty() {
				Self::verify_notary_signature(server_keys, notary_name, notary_keys)?;
			}
		}

		// NOTE: old_verify_keys aren't considered here because old keys can't
		// be used to sign key responses.
		let mut set = ruma::signatures::PublicKeySet::new();
		server_keys.verify_keys.iter().for_each(|(key_id, key)| {
			set.insert(key_id.to_string(), key.key.clone());
		});

		let mut pubkey_map = ruma::signatures::PublicKeyMap::new();
		pubkey_map.insert(server_keys.server_name.to_string(), set);

		let mut canonical = to_canonical_object(server_keys)?;
		strip_extraneous_signatures(&mut canonical, &server_keys.server_name, &[]);
		ruma::signatures::verify_json(&pubkey_map, &canonical).map_err(Into::into)
	}

	/// Verifies that the notary has at least one valid signature present with
	/// the provided signing keys.
	fn verify_notary_signature(
		server_keys: &ServerSigningKeys,
		notary_name: &ServerName,
		notary_keys: &[Base64],
	) -> Result<()> {
		// This strictly verifies that the notary signed this response.
		// This is used to mitigate exploits where a notary's TLS
		// certificate has been compromised, which would otherwise allow
		// them to produce attacker-controlled signing keys with valid
		// self-signatures.
		let Some(notary_signatures) = server_keys.signatures.get(notary_name) else {
			return Err!(Request(Forbidden(
				"Missing notary server signature from {notary_name}"
			)));
		};
		let mut canonical_object = to_canonical_object(notary_signatures)?;
		strip_extraneous_signatures(&mut canonical_object, &server_keys.server_name, &[
			notary_name,
		]);
		let for_verify = ruma::signatures::to_canonical_json_string_for_signing(
			&to_canonical_object(server_keys).expect("server keys object must be canonical json"),
		)
		.expect("canonical JSON must be stringable");

		let raw_notary_keys = notary_keys.iter().map(Base64::as_bytes).collect::<Vec<_>>();
		let raw_notary_signatures = notary_signatures
			.values()
			.flat_map(Base64::<Standard>::parse);

		for signature in raw_notary_signatures {
			let signature_bytes = signature.as_bytes();
			for notary_key in raw_notary_keys.iter().copied() {
				let verified = ruma::signatures::verify_canonical_json_bytes(
					&SigningKeyAlgorithm::Ed25519,
					notary_key,
					signature_bytes,
					for_verify.as_bytes(),
				);
				if verified.is_ok() {
					trace!(
						"Valid signature from {notary_name} present on signing keys response \
						 for {}",
						server_keys.server_name
					);
					return Ok(());
				}
			}
		}

		Err!(Request(Forbidden(
			"No valid signature from {notary_name} present on signing keys response"
		)))
	}
}

pub fn strip_extraneous_signatures(
	canonical: &mut CanonicalJsonObject,
	origin: &ServerName,
	notaries: &[&ServerName],
) {
	canonical.entry("signatures".to_owned()).and_modify(|sigs| {
		sigs.as_object_mut().map(|s| {
			s.retain(|server_name, _| {
				let Ok(server_name) = ServerName::parse(server_name) else { return false };
				origin == server_name || notaries.iter().any(|ns| *ns == server_name)
			});
			Some(s)
		});
	});
}

#[cfg(test)]
#[allow(clippy::unreadable_literal)]
mod tests {
	use ruma::{serde::Raw, server_name};

	use super::*;
	use crate::server_keys::Service;

	#[test]
	fn verify_direct_server_keys_response() {
		let response_data = serde_json::from_value(serde_json::json!(
			{
			  "old_verify_keys": {},
			  "server_name": "continuwuity.org",
			  "signatures": {
				"continuwuity.org": {
				  "ed25519:PwHlNsFu": "YUA+lUu5tQcRhd44bxFcY+WRu8X9nLXT8FUpY44O48/oKF/9rWUhqJGw5uZeOuo2oBMAxtUmO6HT2kBI9WftBA"
				}
			  },
			  "valid_until_ts": 1787690539756_i64,
			  "verify_keys": {
				"ed25519:PwHlNsFu": {
				  "key": "8eNx2s0zWW+heKAmOH5zKv/nCPkEpraDJfGHxDu6hFI"
				}
			  }
			}
		)).map(Raw::from_json).unwrap();

		Service::verify_server_keys_response(&response_data.deserialize().unwrap(), None)
			.expect("Self-signature of real key response is valid");
	}

	#[test]
	fn verify_direct_server_keys_response_ignores_extraneous_signatures() {
		let response_data = serde_json::from_value(serde_json::json!(
			{
			  "old_verify_keys": {},
			  "server_name": "continuwuity.org",
			  "signatures": {
				"continuwuity.org": {
				  "ed25519:PwHlNsFu": "YUA+lUu5tQcRhd44bxFcY+WRu8X9nLXT8FUpY44O48/oKF/9rWUhqJGw5uZeOuo2oBMAxtUmO6HT2kBI9WftBA"
				},
				"example.org": {
				  "ed25519:1": "yadda yadda"
				}
			  },
			  "valid_until_ts": 1787690539756_i64,
			  "verify_keys": {
				"ed25519:PwHlNsFu": {
				  "key": "8eNx2s0zWW+heKAmOH5zKv/nCPkEpraDJfGHxDu6hFI"
				}
			  }
			}
		)).map(Raw::from_json).unwrap();

		Service::verify_server_keys_response(&response_data.deserialize().unwrap(), None)
			.expect("Self-signature of real key response is valid");
	}

	#[test]
	fn verify_notary_response_checks_valid_notary_signature() {
		let response_data = serde_json::from_value(serde_json::json!(
			{
			  "old_verify_keys": {},
			  "server_name": "continuwuity.org",
			  "signatures": {
				"continuwuity.org": {
				  "ed25519:PwHlNsFu": "4YjHGqJ0HN93tvev4kx0qx/3w/h1nlEkFZni15h+5MZ91xUhQNa0BzqdraPU4/KJoXVcOQpmR+hsCVYb2/WlAQ"
				},
				"starstruck.systems": {
				  "ed25519:a_Herg": "G4uXHfn9lizmixPFCBRXVnyf1HB1yyrxPVbtSOdViNHRFrFF66xmEpzDPs341L/Pjxtwfndi4Hw7zIsyTRqoCQ"
				}
			  },
			  "valid_until_ts": 1787691347175_i64,
			  "verify_keys": {
				"ed25519:PwHlNsFu": {
				  "key": "8eNx2s0zWW+heKAmOH5zKv/nCPkEpraDJfGHxDu6hFI"
				}
			  }
			}
		)).map(Raw::from_json).unwrap();
		let starstruck_pubkey =
			Base64::<Standard>::parse("Z4oHuEjdUbtC5W/z/gzc5B8raxNk+8gGwbNwgxHqW8Y").unwrap();
		Service::verify_server_keys_response(
			&response_data.deserialize().unwrap(),
			Some((server_name!("starstruck.systems"), &[starstruck_pubkey])),
		)
		.expect("Server keys from notary are valid");
	}

	#[test]
	fn ensure_verify_server_keys_response_rejects_invalid_self_signature() {
		// even something as small as a mismatch in the timestamp should be
		// enough to trigger this.
		let response_data = serde_json::from_value(serde_json::json!(
			{
			  "old_verify_keys": {},
			  "server_name": "continuwuity.org",
			  "signatures": {
				"continuwuity.org": {
				  "ed25519:PwHlNsFu": "YUA+lUu5tQcRhd44bxFcY+WRu8X9nLXT8FUpY44O48/oKF/9rWUhqJGw5uZeOuo2oBMAxtUmO6HT2kBI9WftBA"
				}
			  },
			  "valid_until_ts": 1787690539757_i64,
			  "verify_keys": {
				"ed25519:PwHlNsFu": {
				  "key": "8eNx2s0zWW+heKAmOH5zKv/nCPkEpraDJfGHxDu6hFI"
				}
			  }
			}
		)).map(Raw::from_json).unwrap();
		Service::verify_server_keys_response(&response_data.deserialize().unwrap(), None)
			.expect_err("Tampered response should fail self-signature check");
	}

	#[test]
	fn ensure_mismatched_notary_signature_fails() {
		// The signature on a notary response is typically useless. However,
		// when additional verification is configured, it can be used to prevent
		// an attack where the notary's TLS is intercepted, and the attacker
		// returns a valid signing key response that is signed by the attacker
		// with the attacker controlled key. In this case, the self-signature
		// would pass, and the attacker would be allowed to inject an
		// adversarial signing key into our database, allowing them to
		// completely forge any event they ever want to as the target.
		// However, by basically doing key pinning, this attack can be mitigated
		// by raising the attack difficulty to requiring both TLS interception,
		// and the target notary's private key.

		// In this case, we'll use a valid self-signature, BUT incorrect
		// notary signature.
		// Without the additional notary validation, this would permit as TOFU.
		let response_data = serde_json::from_value(serde_json::json!(
			{
			  "old_verify_keys": {},
			  "server_name": "continuwuity.org",
			  "signatures": {
				"continuwuity.org": {
				  "ed25519:PwHlNsFu": "Ov3RCRYRUEgaMPfHP4YPUvjoBKE0yVTERNlgeHpxvh/+fknGeRnIZ4e6b0n1aI+gPU04Eb1DE4gaeTFigCudAg"
				}
			  },
			  "valid_until_ts": 1787690539756_i64,
			  "verify_keys": {
				"ed25519:PwHlNsFu": {
				  "key": "8eNx2s0zWW+heKAmOH5zKv/nCPkEpraDJfGHxDu6hFI"
				}
			  }
			}
		)).map(Raw::from_json).unwrap();
		Service::verify_server_keys_response(&response_data.deserialize().unwrap(), None)
			.expect_err("Intercepted notary response should fail strict validation");
	}
}
