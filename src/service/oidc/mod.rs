//! OIDC service.
//!
//! Provides the registrar, authorizer and issuer needed by [api::client::oidc].
//! The whole OAuth2 flow is taken care of by [oxide-auth].
//!
//! TODO this service would need a dedicated space in the database.
//!
//! [oxide-auth]: https://docs.rs/oxide-auth

pub mod registrar;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use conduwuit::{Result, err};
use conduwuit_core::utils;
use oxide_auth::{
	endpoint::Scope,
	frontends::simple::endpoint::{Generic, Vacant},
	primitives::{
		prelude::{
			AuthMap, Authorizer, Client, Issuer, RandomGenerator, Registrar, TokenMap,
		},
		registrar::{EncodedClient, RegisteredUrl},
		grant::Grant,
	}
};
use registrar::MatrixClientMap;
use ruma::{
	api::client::device::Device,
    MilliSecondsSinceUnixEpoch,
    OwnedDeviceId,
    OwnedUserId,
    UserId,
};
use database::{Json, Map};

use crate::{globals, oidc::registrar::MatrixClient, Dep};


pub const SCOPE_PREFIX_DEVICE: &str = "urn:matrix:org.matrix.msc2967.client:device:";
pub const SCOPE_PREFIX_API   : &str = "urn:matrix:org.matrix.msc2967.client:api:";


struct Services {
	globals: Dep<globals::Service>,
}

struct Data {
	userid_devicelistversion: Arc<Map>,
	userdeviceid_metadata: Arc<Map>,
}

pub struct Service {
	registrar: Mutex<MatrixClientMap>,
	authorizer: Mutex<AuthMap<RandomGenerator>>,
	issuer: Mutex<TokenMap<RandomGenerator>>,
	services: Services,
	db: Data,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> { Ok(Arc::new(Self::preconfigured(args))) }

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Register an OAuth client in the registrar (a ClientMap), so it's known by oxide-auth
	/// for future client authentication flows.
	pub fn register_client(&self, name: Option<String>, client: &Client) -> Result<()> {
		self.registrar
			.lock()
			.expect("lockable registrar")
			.register_client(name.clone(), client.clone());

		Ok(())
	}

	/// Register a device in the main continuwuity database. This should only happen on successful
	/// authentication and consent, and will register the client's device_id.
	pub fn register_device(
		&self,
		client_id: &str,
		(user_id, device_id): (&OwnedUserId, &OwnedDeviceId),
		display_name: Option<&str>,
		client_ip: Option<String>,
	) -> Result<()> {
		let key = (user_id, device_id);
		let val = Device {
			device_id: device_id.into(),
			display_name: display_name.map(|n| n.to_string()),
			last_seen_ip: client_ip,
			last_seen_ts: Some(MilliSecondsSinceUnixEpoch::now()),
		};
		increment(&self.db.userid_devicelistversion, user_id.as_bytes());
		self.db.userdeviceid_metadata.put(key, Json(val));
		self
			.registrar
			.lock()
			.expect("lockable registrar")
			.set_client_device_id(client_id, &device_id.to_string())?;

		Ok(())
	}

	#[must_use]
	pub(crate) fn preconfigured(args: crate::Args<'_>) -> Self {
		Self {
			registrar: Mutex::new(
				vec![Client::public(
					"LocalClient",
					RegisteredUrl::Semantic(
						"http://localhost/clientside/endpoint".parse().unwrap(),
					),
					"default-scope".parse().unwrap(),
				)]
				.into_iter()
				.collect(),
			),
			// Authorization tokens are 16 byte random keys to a memory hash map.
			authorizer: Mutex::new(AuthMap::new(RandomGenerator::new(16))),
			// Bearer tokens are also random generated but 256-bit tokens, since they live longer
			// and this example is somewhat paranoid.
			//
			// We could also use a `TokenSigner::ephemeral` here to create signed tokens which can
			// be read and parsed by anyone, but not maliciously created. However, they can not be
			// revoked and thus don't offer even longer lived refresh tokens.
			issuer: Mutex::new(TokenMap::new(RandomGenerator::new(16))),
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
			},
			db: Data {
				userid_devicelistversion: args.db["userid_devicelistversion"].clone(),
				userdeviceid_metadata: args.db["userdeviceid_metadata"].clone(),
			}
		}
	}

	fn grant_from_token(&self, token: &str) -> Option<Grant> {
		let issuer = self.issuer.lock().expect("lockable issuer");

		issuer.recover_token(token).expect("infallible recover_token implementation")
	}

	pub fn client_from_client_id(&self, client_id: &str) -> Option<MatrixClient> {
		let registrar = self.registrar.lock().expect("lockable registrar");

		//registrar.get_client(client_id).map(|mc| mc.client.clone())
		registrar.get_client(client_id).cloned()
	}

	pub fn client_from_device_id(&self, device_id: OwnedDeviceId) -> Option<EncodedClient> {
		let registrar = self.registrar.lock().expect("lockable registrar");

		registrar.find_device(device_id.as_str()).cloned()
	}

	pub fn device_id_from_scope(&self, scope: Scope) -> Result<OwnedDeviceId> {
		let Some(device_id) = scope
			.iter()
			.find(|s| s.starts_with(SCOPE_PREFIX_DEVICE)) else {
				tracing::warn!("device_id not found in scope {scope:?}");
				return Err(err!(Request(InvalidParam("something went wrong with the scope"))));
			};
		let device_id = device_id.replace(SCOPE_PREFIX_DEVICE, "");

		OwnedDeviceId::try_from(device_id.clone())
			.map_err(|err|
				err!(Request(InvalidParam("invalid device_id {device_id:?}: {err}")))
			)
	}

	pub fn user_and_device_from_token(&self, token: &str) -> Result<(OwnedUserId, OwnedDeviceId)> {
		let Some(Grant { owner_id, client_id, .. }) = self.grant_from_token(token) else {
			return Err(err!(Request(MissingToken("unknown token: {token:?}"))));
		};
		let server_name = self.services.globals.server_name();
		let owner_id = UserId::parse_with_server_name(owner_id.clone(), server_name)
			.map_err(|err|
				err!(Request(InvalidUsername("invalid username {owner_id:?}: {err}")))
			)?;
		let client = self.client_from_client_id(&client_id).expect("validated client_id");
		let Some(device_id) = client.device_id else {
			return Err(err!(Request(Unknown("this client has no device_id yet"))));
		};
		let device_id = OwnedDeviceId::from(device_id.to_string());

		Ok((owner_id, device_id))
	}

	/// The oxide-auth carry-all endpoint.
	pub fn endpoint(
		&self,
	) -> Generic<impl Registrar + '_, impl Authorizer + '_, impl Issuer + '_> {
		Generic {
			registrar: self.registrar.lock().unwrap(),
			authorizer: self.authorizer.lock().unwrap(),
			issuer: self.issuer.lock().unwrap(),
			// Solicitor configured later.
			solicitor: Vacant,
			// Scope configured later.
			scopes: Vacant,
			response: Vacant,
		}
	}
}

fn increment(db: &Arc<Map>, key: &[u8]) {
	let old = db.get_blocking(key);
	let new = utils::increment(old.ok().as_deref());
	db.insert(key, new);
}
