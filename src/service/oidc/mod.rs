//! OIDC service.
//!
//! Provides the registrar, authorizer and issuer needed by [api::client::oidc].
//! The whole OIDC OAuth2 flow is taken care of by [oxide-auth].
//!
//! [oxide-auth]: https://docs.rs/oxide-auth

use std::{borrow::Cow, sync::{Arc, Mutex}};

use async_trait::async_trait;
use conduwuit::{err, Result};
use conduwuit_core::utils;
use oxide_auth::{
	endpoint::{PreGrant, Scope},
	frontends::simple::endpoint::{Generic, Vacant},
	primitives::{
		grant::Grant,
		prelude::{
			AuthMap, Authorizer, Client, ClientUrl, Issuer, RandomGenerator, Registrar, TokenMap,
		},
		registrar::{
			Argon2, BoundClient, EncodedClient, RegisteredClient, RegisteredUrl, RegistrarError,
		},
	},
};
use ruma::{
	api::client::device::Device,
    MilliSecondsSinceUnixEpoch,
    OwnedDeviceId,
    OwnedUserId,
    UserId,
};
use database::{Deserialized, Json, Map};
use serde::{Deserialize, Serialize};
use url::Url;
use once_cell::sync::Lazy;

use crate::{globals, Dep};


pub const SCOPE_PREFIX_DEVICE: &str = "urn:matrix:org.matrix.msc2967.client:device:";
pub const SCOPE_PREFIX_API   : &str = "urn:matrix:org.matrix.msc2967.client:api:";

static PASSWORD_POLICY: Lazy<Argon2> = Lazy::new(Argon2::default);


/// A client app that connects to continuwuity via OIDC, as recorded in the
/// database.
#[derive(Clone, Serialize, Deserialize)]
pub struct OidcClient {
	/// The name published by the app itself.
	pub name: Option<String>,
	/// A device id that we'll generate on OIDC registration.
	pub device_id: Option<String>,
	/// The device's coordinates recorded by oxide-auth.
	pub client: EncodedClient,
}

struct Services {
	globals: Dep<globals::Service>,
}

struct Data {
	client_registrar: Arc<Map>,
	deviceid_clientidmap: Arc<Map>,
	userid_devicelistversion: Arc<Map>,
	userdeviceid_metadata: Arc<Map>,
}

pub struct Service {
	/// Authorization tokens are 16 byte random keys to a memory hash map.
	///
	/// Will be reinitialised on continuwuity's restart.
	authorizer: Mutex<AuthMap<RandomGenerator>>,
	/// Bearer tokens are also random generated but 256-bit tokens, since they
	/// live longer.
	///
	/// We could also use a `TokenSigner::ephemeral` here to create signed
	/// tokens which can be read and parsed by anyone, but not maliciously
	/// created. However, they can not be revoked and thus don't offer even
	/// longer lived refresh tokens.
	///
	/// Will be reinitialised on continuwuity's restart.
	issuer: Mutex<TokenMap<RandomGenerator>>,
	services: Services,
	db: Data,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		// TODO implement authorizer and issuer inside database so that token
		// requests survive server restarts.
		Ok(Arc::new(Self {
			authorizer: Mutex::new(AuthMap::new(RandomGenerator::new(16))),
			issuer: Mutex::new(TokenMap::new(RandomGenerator::new(16))),
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
			},
			db: Data {
				client_registrar: args.db["client_registrar"].clone(),
				deviceid_clientidmap: args.db["deviceid_clientidmap"].clone(),
				userid_devicelistversion: args.db["userid_devicelistversion"].clone(),
				userdeviceid_metadata: args.db["userdeviceid_metadata"].clone(),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Register an OIDC client in the client_registrar for future
	/// authentication flows.
	pub fn register_client(&self, display_name: Option<String>, client: &Client) {
		let client = client.clone().encode(&*PASSWORD_POLICY);
		let client_id = client.client_id.clone();
		let oidc_client = OidcClient {
			name: display_name.clone(),
			// Matrix clients have no device_id at registration time.
			device_id: None,
			client,
		};
		self.db
			.client_registrar
			.put(client_id, Json(oidc_client));
	}

	/// Register a device in the main continuwuity database. This should only
	/// happen on successful authentication and consent, and will register the
	/// client's device_id.
	pub async fn register_device(
		&self,
		client_id: &str,
		(user_id, device_id): (&OwnedUserId, &OwnedDeviceId),
		display_name: Option<&str>,
		client_ip: Option<String>,
	) -> Result<()> {
		let device_key = (user_id, device_id);
		let device = Device {
			device_id: device_id.into(),
			display_name: display_name.map(|n| n.to_string()),
			last_seen_ip: client_ip,
			last_seen_ts: Some(MilliSecondsSinceUnixEpoch::now()),
		};
		increment(&self.db.userid_devicelistversion, user_id.as_bytes())
			.await;
		self.db
			.userdeviceid_metadata
			.put(device_key, Json(device));

		let mut client : OidcClient = self.db
			.client_registrar
			.get(client_id)
			.await?
			.deserialized()?;
		client.device_id = Some(device_id.to_string());
		self.db
			.client_registrar
			.put(client_id.to_string(), Json(client));

		self.db
			.deviceid_clientidmap
			.put(device_id, client_id.to_string());

		Ok(())
	}

	fn grant_from_token(&self, token: &str) -> Option<Grant> {
		let issuer = self.issuer.lock().expect("lockable issuer");

		issuer.recover_token(token)
			.expect("infallible recover_token implementation")
	}

	pub async fn client_from_client_id(
		&self,
		client_id: &str,
	) -> Result<Option<OidcClient>> {
		self.db
			.client_registrar
			.get(client_id)
			.await?
			.deserialized()
	}

	pub async fn client_from_device_id(
		&self,
		device_id: OwnedDeviceId,
	) -> Result<Option<OidcClient>> {
		let client_id: String = self.db
			.deviceid_clientidmap
			.get(&device_id)
			.await?
			.deserialized()?;

		self.db
			.client_registrar
			.get(&client_id)
			.await?
			.deserialized()
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

	pub async fn user_and_device_from_token(
		&self,
		token: &str,
	) -> Result<(OwnedUserId, OwnedDeviceId)> {
		let Some(Grant { owner_id, client_id, .. }) = self.grant_from_token(token) else {
			return Err(err!(Request(MissingToken("unknown token: {token:?}"))));
		};
		let server_name = self.services.globals.server_name();
		let owner_id = UserId::parse_with_server_name(owner_id.clone(), server_name)
			.map_err(|err|
				err!(Request(InvalidUsername("invalid username {owner_id:?}: {err}")))
			)?;
		let client = self.client_from_client_id(&client_id)
			.await?
			.expect("validated client_id");
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
			registrar: self,
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

async fn increment(db: &Arc<Map>, key: &[u8]) {
	let old = db.get(key).await;
	let new = utils::increment(old.ok().as_deref());
	db.insert(key, new);
}

/// Substitute "127.0.0.1" and "[::1]" for "localhost" to let oxide-auth compare
/// them ignoring their port.
pub fn normalize_redirect_hostname(url: Url) -> Url {
	let mut new_url = url.clone();
	let new_host = url.host_str().map(|h|
		h.replace("127.0.0.1", "localhost").replace("[::1]", "localhost")
	);
	new_url.set_host(new_host.as_deref()).expect("replaceable redirect hostname");

	new_url
}

/// If `url` is a localhost (either 'localhost', '127.0.0.1' or '[::1]'), wrap
/// it in an `IgnorePortOnLocalhost`, so that oxide-auth ignores the port when
/// comparing it with the registered ones.
pub fn normalize_redirect(url: Url) -> RegisteredUrl {
	let new_url = normalize_redirect_hostname(url);

	match new_url.host_str() {
		Some("localhost") => RegisteredUrl::IgnorePortOnLocalhost(new_url.into()),
		_ => RegisteredUrl::Semantic(new_url)
	}
}

/// Let this service act as an oxide-auth `Registrar`.
impl Registrar for Service {
    fn bound_redirect<'a>(&self, bound: ClientUrl<'a>) -> Result<BoundClient<'a>, RegistrarError> {
		let client_handle = self.db
			.client_registrar
			.get_blocking(bound.client_id.as_ref())
			.map_err(|_| RegistrarError::Unspecified)?;
		let oidc_client: OidcClient = client_handle.deserialized()
			.map_err(|_| RegistrarError::Unspecified)?;
		let client = oidc_client.client;
        // Perform exact matching as motivated in the rfc, but substitute
		// "127.0.0.1" and "[::1]" for "localhost" to let oxide-auth ignore
		// their port.
		let redirect_uri = bound.redirect_uri;
		let normalized_uri = redirect_uri
			.clone()
			.map(|u| normalize_redirect(u.to_url()));
        let redirect_uri = match normalized_uri {
            None => client.redirect_uri.clone(),
            Some(url) => {
                let original = std::iter::once(&client.redirect_uri);
                let alternatives = client.additional_redirect_uris.iter();
                if original
                    .chain(alternatives)
                    .any(|registered| *registered == url)
                {
					// If normalized_uri is Some(url), so is redirect_uri, so unwrap().
                    redirect_uri.unwrap().into_owned().into()
                } else {
					tracing::debug!("the request's redirect url didn't match any registered. bound: {:?}, in client {:#?}", url, client);
                    return Err(RegistrarError::Unspecified);
                }
            }
        };

        Ok(BoundClient {
            client_id: bound.client_id,
            redirect_uri: Cow::Owned(redirect_uri),
        })
	}

    fn negotiate(&self, bound: BoundClient<'_>, _scope: Option<Scope>) -> Result<PreGrant, RegistrarError> {
		let client_handle = self.db
			.client_registrar
			.get_blocking(bound.client_id.as_ref())
			.map_err(|_| RegistrarError::Unspecified)?;
		let oidc_client: OidcClient = client_handle.deserialized()
			.map_err(|_| RegistrarError::Unspecified)?;

        Ok(PreGrant {
            client_id: bound.client_id.into_owned(),
            redirect_uri: bound.redirect_uri.into_owned(),
			// Always use the client's scope.
            scope: oidc_client.client.default_scope.clone(),
        })
    }

    fn check(&self, client_id: &str, passphrase: Option<&[u8]>) -> Result<(), RegistrarError> {
		let client_handle = self.db
			.client_registrar
			.get_blocking(client_id)
			.map_err(|_| RegistrarError::Unspecified)?;
		let oidc_client: OidcClient = client_handle.deserialized()
			.map_err(|_| RegistrarError::Unspecified)?;
		let client = oidc_client.client;

		RegisteredClient::new(&client, &*PASSWORD_POLICY)
			.check_authentication(passphrase)
    }
}
