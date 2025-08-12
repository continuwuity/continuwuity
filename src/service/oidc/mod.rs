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
use oxide_auth::{
	frontends::simple::endpoint::{Generic, Vacant},
	primitives::{
		grant::Grant,
		prelude::{
			AuthMap, Authorizer, Client, Issuer, RandomGenerator, Registrar, TokenMap,
		},
		registrar::RegisteredUrl,
	},
};
use registrar::ClientMap;
use ruma::{OwnedDeviceId, OwnedUserId, UserId};

use crate::{globals, Dep};

struct Services {
	globals: Dep<globals::Service>,
}

pub struct Service {
	registrar: Mutex<ClientMap>,
	authorizer: Mutex<AuthMap<RandomGenerator>>,
	issuer: Mutex<TokenMap<RandomGenerator>>,
	services: Services,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> { Ok(Arc::new(Self::preconfigured(args))) }

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	pub fn register_client(&self, client: &Client) -> Result<()> {
		self.registrar
			.lock()
			.expect("lockable registrar")
			.register_client(client.clone());

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
		}
	}

	fn grant_from_token(&self, token: &str) -> Option<Grant> {
		let issuer = self.issuer.lock().expect("lockable issuer");

		issuer.recover_token(token).expect("infallible recover_token implementation")
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
		let device_id = OwnedDeviceId::try_from(client_id.clone())
			.map_err(|err|
				err!(Request(InvalidParam("invalid client_id {client_id:?}: {err}")))
			)?;

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
