use std::borrow::Cow;
use url::Url;
use std::collections::HashMap;
use std::iter::{Extend, FromIterator};

use conduwuit::{Result, err};
use oxide_auth::endpoint::{PreGrant, Scope};
use oxide_auth::primitives::prelude::{Client, ClientUrl};
use oxide_auth::primitives::registrar::{Argon2, BoundClient, EncodedClient, PasswordPolicy, RegisteredClient, RegisteredUrl, Registrar, RegistrarError};
use once_cell::sync::Lazy;

/// Substitute "127.0.0.1" and "[::1]" for "localhost" to let oxide-auth compare them
/// ignoring their port.
pub fn normalize_redirect_hostname(url: Url) -> Url {
	let mut new_url = url.clone();
	let new_host = url.host_str().map(|h|
		h.replace("127.0.0.1", "localhost").replace("[::1]", "localhost")
	);
	new_url.set_host(new_host.as_deref()).expect("replaceable redirect hostname");

	new_url
}

/// If `url` is a localhost (either 'localhost', '127.0.0.1' or '[::1]'), wrap it in an
/// IgnorePortOnLocalhost, so that oxide-auth ignores the port when comparing it with the
/// registered ones.
pub fn normalize_redirect(url: Url) -> RegisteredUrl {
	let new_url = normalize_redirect_hostname(url);

	match new_url.host_str() {
		Some("localhost") => RegisteredUrl::IgnorePortOnLocalhost(new_url.into()),
		_ => RegisteredUrl::Semantic(new_url)
	}
}


static DEFAULT_PASSWORD_POLICY: Lazy<Argon2> = Lazy::new(Argon2::default);

#[derive(Clone)]
pub struct MatrixClient {
	pub name: Option<String>,
	pub device_id: Option<String>,
	pub client: EncodedClient,
}

/// A very simple, in-memory hash map of client ids to MatrixClient entries.
#[derive(Default)]
pub struct MatrixClientMap {
    clients: HashMap<String, MatrixClient>,
    password_policy: Option<Box<dyn PasswordPolicy>>,
}

impl MatrixClientMap {
    /// Create an empty map without any clients in it.
    pub fn new() -> MatrixClientMap {
        MatrixClientMap::default()
    }

    /// Insert or update the client record with an oxide-auth OIDC Client.
	/// This should only be called from the OIDC register endpoint.
    pub fn register_client(&mut self, name: Option<String>, client: Client) {
        let password_policy = Self::current_policy(&self.password_policy);
		let client = client.encode(password_policy);
		// Matrix clients have no device_id at registration time.
		let matrix_client = MatrixClient { name, device_id: None, client: client.clone() };
		self.clients.insert(client.client_id.clone(), matrix_client);
    }

	/// Add `device_id` to client stored at `client_id`.
	pub fn set_client_device_id(&mut self, client_id: &str, device_id: &str) -> Result<()> {
		let client = self.clients.get_mut(client_id).ok_or_else(||
			err!(Request(Unknown("a client under client_id"))))?;
		client.device_id = Some(device_id.to_string());

		Ok(())
	}

	/// Returns a MatrixClient, containing an oxide-auth EncodedClient, and some metadata,
	/// like a public name.
	pub fn get_client(&self, client_id: &str) -> Option<&MatrixClient> {
		self.clients.get(client_id)
	}

	pub fn find_device(&self, device_id: &str) -> Option<&EncodedClient> {
		self.clients
			.values()
			.find(|c| c.device_id.as_deref().is_some_and(|d| d == device_id))
			.map(|c| &c.client)
	}

    /// Change how passwords are encoded while stored.
    pub fn set_password_policy<P: PasswordPolicy + 'static>(&mut self, new_policy: P) {
        self.password_policy = Some(Box::new(new_policy))
    }

	pub fn get_redirect(&self, client: Client) -> RegisteredUrl {
        let password_policy = Self::current_policy(&self.password_policy);
		let client = client.encode(password_policy);

		client.redirect_uri
	}

    // This is not an instance method because it needs to borrow the box but register needs &mut
    fn current_policy<'a>(policy: &'a Option<Box<dyn PasswordPolicy>>) -> &'a dyn PasswordPolicy {
        policy
            .as_ref()
            .map(|boxed| &**boxed)
            .unwrap_or(&*DEFAULT_PASSWORD_POLICY)
    }
}

impl Extend<Client> for MatrixClientMap {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Client>,
    {
        iter.into_iter().for_each(|client| self.register_client(None, client))
    }
}

impl FromIterator<Client> for MatrixClientMap {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Client>,
    {
        let mut into = MatrixClientMap::new();
        into.extend(iter);
        into
    }
}

impl Registrar for MatrixClientMap {
    fn bound_redirect<'a>(&self, bound: ClientUrl<'a>) -> Result<BoundClient<'a>, RegistrarError> {
        let client = match self.clients.get(bound.client_id.as_ref()) {
            None => {
				tracing::debug!("this client was not registered: {}", bound.client_id);
				return Err(RegistrarError::Unspecified);
			},
            Some(stored) => &stored.client,
        };

        // Perform exact matching as motivated in the rfc, but substitute "127.0.0.1" and
		// "[::1]" for "localhost" to let oxide-auth ignore their port.
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
        let client = &self
            .clients
            .get(bound.client_id.as_ref())
            .expect("Bound client appears to not have been constructed with this registrar")
			.client;
		// Always take the client's scope.
        Ok(PreGrant {
            client_id: bound.client_id.into_owned(),
            redirect_uri: bound.redirect_uri.into_owned(),
            scope: client.default_scope.clone(),
        })
    }

    fn check(&self, client_id: &str, passphrase: Option<&[u8]>) -> Result<(), RegistrarError> {
        let password_policy = Self::current_policy(&self.password_policy);

        self.clients
            .get(client_id)
			.ok_or_else(|| {
				tracing::debug!("this client is not registered yet: {client_id:?}.");
				RegistrarError::Unspecified
			}).and_then(|mc|
				RegisteredClient::new(&mc.client, password_policy).check_authentication(passphrase)
			)?;

        Ok(())
    }
}

