mod get;
mod keypair;
mod request;
mod sign;
mod util;
mod verify;

use std::{collections::BTreeMap, sync::Arc};

use conduwuit::{
	Result, Server,
	utils::{IterStream, ReadyExt, stream::TryIgnore},
};
use database::{Deserialized, Ignore, Interfix, Json, Map};
use futures::{Stream, StreamExt};
pub use request::in_one_week;
use ruma::{
	CanonicalJsonObject, MilliSecondsSinceUnixEpoch, OwnedServerSigningKeyId, ServerName,
	ServerSigningKeyId,
	api::federation::discovery::{ServerSigningKeys, VerifyKey},
	room_version_rules::RoomVersionRules,
	signatures::{Ed25519KeyPair, PublicKeyMap, PublicKeySet},
};
pub use verify::strip_extraneous_signatures;

use crate::{Dep, globals, sending, server_keys::util::required_keys};

pub struct Service {
	keypair: Box<Ed25519KeyPair>,
	verify_keys: VerifyKeys,
	services: Services,
	db: Data,
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
		}))
	}

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
}
