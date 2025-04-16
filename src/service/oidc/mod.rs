/// OIDC service.
///
/// Provides the registrar, authorizer and issuer needed by [api::client::oidc].
/// The whole OAuth2 flow is taken care of by [oxide-auth].
///
/// TODO At the moment this service provides no method to dynamically add a
/// client. That would need a dedicated space in the database.
///
/// [oxide-auth]: https://docs.rs/oxide-auth

use conduwuit::Result;
use oxide_auth::{
	frontends::simple::endpoint::{Generic, Vacant},
	primitives::{
		prelude::{
			AuthMap,
			Authorizer,
			Client,
			ClientMap,
			Issuer,
			RandomGenerator,
			Registrar,
			TokenMap,
		},
		registrar::RegisteredUrl,
	},
};

use async_trait::async_trait;
use std::sync::{Arc, Mutex};

pub struct Service {
	registrar: Mutex<ClientMap>,
	authorizer: Mutex<AuthMap<RandomGenerator>>,
	issuer: Mutex<TokenMap<RandomGenerator>>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(_args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self::preconfigured()))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	pub fn preconfigured() -> Self {
		Service {
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
		}
	}

	/// The oxide-auth carry-all endpoint.
	pub fn endpoint(&self) -> Generic<impl Registrar + '_, impl Authorizer + '_, impl Issuer + '_> {
		Generic {
			registrar: self.registrar.lock().unwrap(),
			authorizer: self.authorizer.lock().unwrap(),
			issuer: self.issuer.lock().unwrap(),
			// Solicitor configured later.
			solicitor: Vacant,
			// Scope configured later.
			scopes: Vacant,
			// `rocket::Response` is `Default`, so we don't need more configuration.
			response: Vacant,
		}
	}
}
