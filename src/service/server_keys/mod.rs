mod get;
mod keypair;
mod request;
mod sign;
mod util;
mod verify;

use std::{
	collections::{BTreeMap, HashMap},
	sync::Arc,
	time::{Duration, Instant},
};

use assign::assign;
use async_trait::async_trait;
use conduwuit::{
	Result, Server, SyncRwLock,
	utils::{IterStream, ReadyExt, stream::TryIgnore, timepoint_from_now, to_canonical_object},
};
use database::{Deserialized, Ignore, Interfix, Json, Map};
use futures::{Stream, StreamExt};
pub use request::in_one_week;
use ruma::{
	CanonicalJsonObject, MilliSecondsSinceUnixEpoch, OwnedServerName, OwnedServerSigningKeyId,
	ServerName, ServerSigningKeyId,
	api::federation::discovery::{OldVerifyKey, ServerSigningKeys, VerifyKey},
	room_version_rules::RoomVersionRules,
	serde::Raw,
	signatures::{Ed25519KeyPair, PublicKeyMap, PublicKeySet},
};
use serde_json::value::to_raw_value;
pub use verify::strip_extraneous_signatures;

use crate::{Dep, globals, sending, server_keys::util::required_keys};

const VALID_UNTIL_TS_DURATION: Duration = Duration::from_hours(12);

pub struct Service {
	keypair: Box<Ed25519KeyPair>,
	verify_keys: VerifyKeys,
	services: Services,
	db: Data,
	last_lookup: SyncRwLock<HashMap<OwnedServerName, Instant>>,
}

struct Services {
	globals: Dep<globals::Service>,
	sending: Dep<sending::Service>,
	server: Arc<Server>,
}

struct Data {
	servernamekeyid_response: Arc<Map>,
}

pub type VerifyKeys = BTreeMap<OwnedServerSigningKeyId, VerifyKey>;
pub type PubKeyMap = PublicKeyMap;
pub type PubKeys = PublicKeySet;

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let (keypair, verify_keys) = keypair::init(args.db)?;
		debug_assert!(verify_keys.len() == 1, "only one active verify_key supported");

		Ok(Arc::new(Self {
			keypair,
			verify_keys,
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				sending: args.depend::<sending::Service>("sending"),
				server: args.server.clone(),
			},
			db: Data {
				servernamekeyid_response: args.db["servernamekeyid_response"].clone(),
			},
			last_lookup: SyncRwLock::new(HashMap::new()),
		}))
	}

	async fn clear_cache(&self) { self.last_lookup.write().clear(); }

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Fetches the server's active keypair.
	pub fn keypair(&self) -> &Ed25519KeyPair { &self.keypair }

	/// Fetches the server's active signing key. Panics if there's more than 1
	/// active key.
	pub fn active_verify_key(&self) -> (OwnedServerSigningKeyId, VerifyKey) {
		debug_assert!(self.verify_keys.len() <= 1, "more than one active verify_key");
		self.verify_keys
			.iter()
			.next()
			.map(|(id, key)| (id.to_owned(), key.to_owned()))
			.expect("missing active verify_key")
	}

	/// Adds a signing key response to the database.
	fn add_signing_keys(&self, new_keys: &ServerSigningKeys) {
		// TODO(nex): this results in massive amounts of data duplication.
		// Perhaps a sliding approach would be better? where we just update the
		// last known response if the only thing that changed was the
		// valid_until_ts. This would require a more complex data structure
		// though. For now, just storing the duplicated data is fine.
		for key in new_keys
			.verify_keys
			.keys()
			.chain(new_keys.old_verify_keys.keys())
		{
			let db_key = (new_keys.server_name.as_str(), key.as_str());
			self.db.servernamekeyid_response.put(db_key, Json(new_keys));
		}
	}

	/// Checks if the keys required to verify an incoming PDU are already known
	/// locally.
	pub async fn required_keys_exist(
		&self,
		object: &CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> bool {
		let Ok(required_keys) = required_keys(object, &room_version_rules.signatures) else {
			return false;
		};
		let Ok(min_valid_ts) = Self::min_valid_ts_from_event(object) else {
			return false;
		};
		required_keys
			.iter()
			.flat_map(|(server, key_ids)| key_ids.iter().map(move |key_id| (server, key_id)))
			.stream()
			.all(|(server, key_id)| self.verify_key_exists(server, key_id, min_valid_ts))
			.await
	}

	/// Checks if a single verify key belonging to the target server is already
	/// known locally.
	pub async fn verify_key_exists(
		&self,
		origin: &ServerName,
		key_id: &ServerSigningKeyId,
		minimum_valid_ts: MilliSecondsSinceUnixEpoch,
	) -> bool {
		let Some(keys) = self.get_signing_key(origin, key_id).await else {
			// We have no keys for this server
			return false;
		};

		if keys.valid_until_ts < minimum_valid_ts || keys.valid_until_ts > in_one_week() {
			// This key response is stale and cannot be trusted.
			return false;
		}

		if keys.verify_keys.contains_key(key_id) {
			// An in-date, active key exists for this server.
			return true;
		}

		if let Some(expired_key) = keys.old_verify_keys.get(key_id) {
			// This key can still be used to verify signatures provided it
			// expired within the minimum valid time window.
			return expired_key.expired_ts.min(keys.valid_until_ts) >= minimum_valid_ts;
		}

		false
	}

	pub async fn get_signing_key(
		&self,
		origin: &ServerName,
		key_id: &ServerSigningKeyId,
	) -> Option<ServerSigningKeys> {
		self.db
			.servernamekeyid_response
			.qry(&(origin.as_str(), key_id.as_str()))
			.await
			.deserialized::<ServerSigningKeys>()
			.ok()
	}

	/// Returns all (active) verify keys for the origin.
	pub async fn verify_keys_for(&self, origin: &ServerName) -> VerifyKeys {
		let mut keys = self
			.signing_keys_for(origin)
			.ready_fold(BTreeMap::new(), |mut base, keys| {
				base.extend(keys.verify_keys);
				base
			})
			.await;

		if self.services.globals.server_is_ours(origin) {
			keys.extend(self.verify_keys.clone());
		}

		keys
	}

	/// Returns the stored server signing keys responses for the origin.
	///
	/// Does not imply that the stored responses are still in-date.
	pub fn signing_keys_for<'a>(
		&'a self,
		origin: &'a ServerName,
	) -> impl Stream<Item = ServerSigningKeys> + Send + 'a {
		self.db
			.servernamekeyid_response
			.stream_prefix(&(origin, Interfix))
			.ignore_err()
			.map(|(_, v): (Ignore, ServerSigningKeys)| v)
	}

	/// Determines if the server may contact the origin server to fetch keys
	/// when acting as a notary server. This limits origin lookups to once per
	/// minute, which prevents amplification attacks.
	#[must_use]
	pub fn notary_may_contact_origin(&self, server_name: &ServerName) -> bool {
		if server_name == self.services.server.name {
			// We always intercept origin requests to us with
			// build_server_keys_response, so this is equivalent to
			// calling `/_matrix/key/v2/server`.
			return true;
		}
		self.last_lookup
			.read()
			.get(server_name)
			.is_none_or(|last| last.elapsed() >= Duration::from_mins(1))
	}

	/// Generates a signing key response to serve to `/_matrix/key/v2/server`.
	pub async fn build_server_keys_response(&self) -> Result<Raw<ServerSigningKeys>> {
		let verify_keys = BTreeMap::from([self.active_verify_key()]);
		let old_verify_keys = match &self.services.server.config.old_verify_keys {
			| Some(old_verify_keys) => old_verify_keys.to_owned(),
			| None => self.discover_old_signing_keys().await,
		};
		let valid_until_ts = timepoint_from_now(VALID_UNTIL_TS_DURATION)
			.map(|tp| {
				MilliSecondsSinceUnixEpoch::from_system_time(tp)
					.expect("key validity period must be before the heat death of the universe")
			})
			.expect("key validity period must be before the heat death of the universe");
		let server_keys = assign!(
			ServerSigningKeys::new(self.services.server.name.clone(), valid_until_ts),
			{verify_keys, old_verify_keys}
		);
		let mut canonical_obj = to_canonical_object(&server_keys)?;
		self.sign_json(&mut canonical_obj)
			.and_then(|()| to_raw_value(&canonical_obj).map_err(Into::into))
			.map(Raw::from_json)
	}

	async fn discover_old_signing_keys(&self) -> BTreeMap<OwnedServerSigningKeyId, OldVerifyKey> {
		let now = MilliSecondsSinceUnixEpoch::now();
		let mut keys = self.signing_keys_for(&self.services.server.name);
		let mut old_keys = BTreeMap::new();
		while let Some(resp) = keys.next().await {
			// We need to verify that this response is still trusted based on
			// the current configuration. It's possible we got this response
			// from a notary who we no longer trust.
			if resp.valid_until_ts > in_one_week() || resp.valid_until_ts < now {
				continue;
			}
			let trusted = self
				.services
				.server
				.config
				.trusted_servers
				.iter()
				.any(|notary| {
					Self::verify_server_keys_response(
						&resp,
						Some((notary.server_name(), notary.verify_keys())),
					)
					.is_ok()
				});
			if !trusted {
				continue;
			}
			for (old_key_id, old_key) in resp.old_verify_keys {
				if old_key.expired_ts > resp.valid_until_ts {
					// This key expired in the future which is probably illegal
					continue;
				}
				old_keys
					.entry(old_key_id)
					.and_modify(|current_old_key: &mut OldVerifyKey| {
						// Use the lowest expiry timestamp
						if current_old_key.expired_ts < old_key.expired_ts {
							*current_old_key = old_key.clone();
						}
					})
					.or_insert(old_key);
			}
		}

		old_keys
	}
}
