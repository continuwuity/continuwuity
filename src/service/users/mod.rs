pub(super) mod account;
pub(super) mod dehydrated_device;
pub(super) mod device;
pub(super) mod filters;
pub(super) mod keys;
pub(super) mod profile;
pub(super) mod remote;

use std::{mem, sync::Arc};

pub use account::{AccessTokenStatus, AccountStatus};
use conduwuit::{
	Err, Error, Result, err,
	utils::{self},
};
use database::Map;
pub use device::DeviceToken;
pub use profile::ProfileFieldChange;
use ruma::{UserId, api::error::ErrorKind, encryption::CrossSigningKey, serde::Raw};
use serde::{Deserialize, Serialize};

use crate::{
	Dep, account_data, admin, appservice, config, firstrun, globals, oauth, presence,
	rooms::{self, alias, membership},
	sync, threepid,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSuspension {
	/// Whether the user is currently suspended
	pub suspended: bool,
	/// When the user was suspended (Unix timestamp in milliseconds)
	pub suspended_at: u64,
	/// User ID of who suspended this user
	pub suspended_by: String,
}

/// A password hash. This is only for use when setting a user's password,
/// if the hash needs to be kept around for a while without keeping the password
/// in memory.
#[derive(Serialize, Deserialize)]
pub struct HashedPassword(String);

impl HashedPassword {
	pub fn new(password: &str) -> Result<Self> {
		Ok(Self(utils::hash::password(password).map_err(|e| {
			err!(Request(InvalidParam("Password does not meet the requirements: {e}")))
		})?))
	}
}

pub struct Service {
	services: Services,
	db: Data,
}

struct Services {
	account_data: Dep<account_data::Service>,
	admin: Dep<admin::Service>,
	alias: Dep<alias::Service>,
	appservice: Dep<appservice::Service>,
	config: Dep<config::Service>,
	firstrun: Dep<firstrun::Service>,
	globals: Dep<globals::Service>,
	membership: Dep<membership::Service>,
	oauth: Dep<oauth::Service>,
	presence: Dep<presence::Service>,
	state: Dep<rooms::state::Service>,
	state_accessor: Dep<rooms::state_accessor::Service>,
	state_cache: Dep<rooms::state_cache::Service>,
	sync: Dep<sync::Service>,
	threepid: Dep<threepid::Service>,
	timeline: Dep<rooms::timeline::Service>,
}

struct Data {
	keychangeid_userid: Arc<Map>,
	keyid_key: Arc<Map>,
	onetimekeyid_onetimekeys: Arc<Map>,
	fallbackkeyid_fallbackkey: Arc<Map>,
	openidtoken_expiresatuserid: Arc<Map>,
	logintoken_expiresatuserid: Arc<Map>,
	todeviceid_events: Arc<Map>,
	token_userdeviceid: Arc<Map>,
	remoteuserid_remoteuser: Arc<Map>,
	userdeviceid_tokenexpires: Arc<Map>,
	userdeviceid_metadata: Arc<Map>,
	userdeviceid_token: Arc<Map>,
	userfilterid_filter: Arc<Map>,
	userid_avatarurl: Arc<Map>,
	userid_deactivated: Arc<Map>,
	userid_dehydrateddevice: Arc<Map>,
	userid_devicelistversion: Arc<Map>,
	userid_displayname: Arc<Map>,
	userid_lastonetimekeyupdate: Arc<Map>,
	userid_masterkeyid: Arc<Map>,
	userid_password: Arc<Map>,
	userid_suspension: Arc<Map>,
	userid_lock: Arc<Map>,
	userid_logindisabled: Arc<Map>,
	userid_selfsigningkeyid: Arc<Map>,
	userid_usersigningkeyid: Arc<Map>,
	useridprofilekey_value: Arc<Map>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				account_data: args.depend::<account_data::Service>("account_data"),
				admin: args.depend::<admin::Service>("admin"),
				alias: args.depend::<alias::Service>("alias"),
				appservice: args.depend::<appservice::Service>("appservice"),
				config: args.depend::<config::Service>("config"),
				firstrun: args.depend::<firstrun::Service>("firstrun"),
				globals: args.depend::<globals::Service>("globals"),
				membership: args.depend::<membership::Service>("membership"),
				oauth: args.depend::<oauth::Service>("oauth"),
				presence: args.depend::<presence::Service>("presence"),
				state: args.depend::<rooms::state::Service>("rooms::state"),
				state_accessor: args
					.depend::<rooms::state_accessor::Service>("rooms::state_accessor"),
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
				sync: args.depend::<sync::Service>("sync"),
				threepid: args.depend::<threepid::Service>("threepid"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
			},
			db: Data {
				keychangeid_userid: args.db["keychangeid_userid"].clone(),
				keyid_key: args.db["keyid_key"].clone(),
				onetimekeyid_onetimekeys: args.db["onetimekeyid_onetimekeys"].clone(),
				fallbackkeyid_fallbackkey: args.db["fallbackkeyid_fallbackkey"].clone(),
				openidtoken_expiresatuserid: args.db["openidtoken_expiresatuserid"].clone(),
				logintoken_expiresatuserid: args.db["logintoken_expiresatuserid"].clone(),
				todeviceid_events: args.db["todeviceid_events"].clone(),
				token_userdeviceid: args.db["token_userdeviceid"].clone(),
				remoteuserid_remoteuser: args.db["remoteuserid_remoteuser"].clone(),
				userdeviceid_metadata: args.db["userdeviceid_metadata"].clone(),
				userdeviceid_token: args.db["userdeviceid_token"].clone(),
				userfilterid_filter: args.db["userfilterid_filter"].clone(),
				userid_avatarurl: args.db["userid_avatarurl"].clone(),
				userid_deactivated: args.db["userid_deactivated"].clone(),
				userid_dehydrateddevice: args.db["userid_dehydrateddevice"].clone(),
				userid_devicelistversion: args.db["userid_devicelistversion"].clone(),
				userid_displayname: args.db["userid_displayname"].clone(),
				userid_lastonetimekeyupdate: args.db["userid_lastonetimekeyupdate"].clone(),
				userid_masterkeyid: args.db["userid_masterkeyid"].clone(),
				userid_password: args.db["userid_password"].clone(),
				userid_suspension: args.db["userid_suspension"].clone(),
				userid_lock: args.db["userid_lock"].clone(),
				userid_logindisabled: args.db["userid_logindisabled"].clone(),
				userid_selfsigningkeyid: args.db["userid_selfsigningkeyid"].clone(),
				userid_usersigningkeyid: args.db["userid_usersigningkeyid"].clone(),
				useridprofilekey_value: args.db["useridprofilekey_value"].clone(),
				userdeviceid_tokenexpires: args.db["userdeviceid_tokenexpires"].clone(),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

pub fn parse_master_key(
	user_id: &UserId,
	master_key: &Raw<CrossSigningKey>,
) -> Result<(Vec<u8>, CrossSigningKey)> {
	let mut prefix = user_id.as_bytes().to_vec();
	prefix.push(0xFF);

	let master_key = master_key
		.deserialize()
		.map_err(|_| Error::BadRequest(ErrorKind::InvalidParam, "Invalid master key"))?;
	let mut master_key_ids = master_key.keys.values();
	let master_key_id = master_key_ids
		.next()
		.ok_or(Error::BadRequest(ErrorKind::InvalidParam, "Master key contained no key."))?;
	if master_key_ids.next().is_some() {
		return Err(Error::BadRequest(
			ErrorKind::InvalidParam,
			"Master key contained more than one key.",
		));
	}
	let mut master_key_key = prefix.clone();
	master_key_key.extend_from_slice(master_key_id.as_bytes());
	Ok((master_key_key, master_key))
}

pub fn parse_user_signing_key(user_signing_key: &Raw<CrossSigningKey>) -> Result<String> {
	let mut user_signing_key_ids = user_signing_key
		.deserialize()
		.map_err(|_| err!(Request(InvalidParam("Invalid user signing key"))))?
		.keys
		.into_values();

	let user_signing_key_id = user_signing_key_ids
		.next()
		.ok_or(err!(Request(InvalidParam("User signing key contained no key."))))?;

	if user_signing_key_ids.next().is_some() {
		return Err!(Request(InvalidParam("User signing key contained more than one key.")));
	}

	Ok(user_signing_key_id)
}

/// Ensure that a user only sees signatures from themselves and the target user
fn clean_signatures<F>(
	mut cross_signing_key: serde_json::Value,
	sender_user: Option<&UserId>,
	user_id: &UserId,
	allowed_signatures: &F,
) -> Result<serde_json::Value>
where
	F: Fn(&UserId) -> bool + Send + Sync,
{
	if let Some(signatures) = cross_signing_key
		.get_mut("signatures")
		.and_then(|v| v.as_object_mut())
	{
		// Don't allocate for the full size of the current signatures, but require
		// at most one resize if nothing is dropped
		let new_capacity = signatures.len() / 2;
		for (user, signature) in
			mem::replace(signatures, serde_json::Map::with_capacity(new_capacity))
		{
			let sid = <&UserId>::try_from(user.as_str())
				.map_err(|_| Error::bad_database("Invalid user ID in database."))?;
			if sender_user == Some(user_id) || sid == user_id || allowed_signatures(sid) {
				signatures.insert(user, signature);
			}
		}
	}

	Ok(cross_signing_key)
}

//TODO: this is an ABA
fn increment(db: &Arc<Map>, key: &[u8]) {
	let old = db.get_blocking(key);
	let new = utils::increment(old.ok().as_deref());
	db.insert(key, new);
}
