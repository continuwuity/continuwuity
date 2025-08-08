use std::borrow::Cow;
use url::Url;
use std::collections::HashMap;
use std::iter::{Extend, FromIterator};

use oxide_auth::endpoint::{PreGrant, Scope};
use oxide_auth::primitives::prelude::{Client, ClientUrl};
use oxide_auth::primitives::registrar::{Argon2, BoundClient, EncodedClient, PasswordPolicy, RegisteredClient, RegisteredUrl, Registrar, RegistrarError};
use once_cell::sync::Lazy;

/// oxide-auth can only ignore ports on localhost if it's spelled "localhost",
/// not "127.0.0.1" or "[::1]". This function does that replacement.
pub fn normalize_redirect_hostname(url: Url) -> Url {
	let mut new_url = url.clone();
	let new_host = url.host_str().map(|h|
		h.replace("127.0.0.1", "localhost").replace("[::1]", "localhost")
	);
	new_url.set_host(new_host.as_deref()).expect("replaceable redirect hostname");

	new_url
}

/// The redirect_uri has to be wrapped in an IgnorePortOnLocalhost for oxide-auth
/// to ignore the port when comparing it with the registered ones.
pub fn normalize_redirect(url: Url) -> RegisteredUrl {
	let new_url = normalize_redirect_hostname(url);

	match new_url.host_str() {
		Some("localhost") => RegisteredUrl::IgnorePortOnLocalhost(new_url.into()),
		_ => RegisteredUrl::Semantic(new_url)
	}
}


static DEFAULT_PASSWORD_POLICY: Lazy<Argon2> = Lazy::new(Argon2::default);

/// A very simple, in-memory hash map of client ids to Client entries.
#[derive(Default)]
pub struct ClientMap {
    clients: HashMap<String, EncodedClient>,
    password_policy: Option<Box<dyn PasswordPolicy>>,
}

impl ClientMap {
    /// Create an empty map without any clients in it.
    pub fn new() -> ClientMap {
        ClientMap::default()
    }

    /// Insert or update the client record.
    pub fn register_client(&mut self, client: Client) {
        let password_policy = Self::current_policy(&self.password_policy);
		let client = client.encode(password_policy);
		self.clients.insert(client.client_id.clone(), client);
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

impl Extend<Client> for ClientMap {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Client>,
    {
        iter.into_iter().for_each(|client| self.register_client(client))
    }
}

impl FromIterator<Client> for ClientMap {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Client>,
    {
        let mut into = ClientMap::new();
        into.extend(iter);
        into
    }
}

impl Registrar for ClientMap {
    fn bound_redirect<'a>(&self, bound: ClientUrl<'a>) -> Result<BoundClient<'a>, RegistrarError> {
        let client = match self.clients.get(bound.client_id.as_ref()) {
            None => {
				tracing::debug!("this client was not registered: {}", bound.client_id);
				return Err(RegistrarError::Unspecified);
			},
            Some(stored) => stored,
        };

        // Perform exact matching as motivated in the rfc, just substitute "127.0.0.1" and
		// "[::1]" for "localhost".
		let redirect_uri = bound.redirect_uri
			.map(|u| normalize_redirect(u.to_url()));
        let registered_url = match redirect_uri {
            None => client.redirect_uri.clone(),
            Some(url) => {
                let original = std::iter::once(&client.redirect_uri);
                let alternatives = client.additional_redirect_uris.iter();
                if original
                    .chain(alternatives)
                    .any(|registered| *registered == url)
                {
                    url.clone().into()
                } else {
					tracing::debug!("the request's redirect url didn't match any registered. bound: {:?}, in client {:#?}", url, client);
                    return Err(RegistrarError::Unspecified);
                }
            }
        };

        Ok(BoundClient {
            client_id: bound.client_id,
            redirect_uri: Cow::Owned(registered_url),
        })
    }

    /// Always overrides the scope with a default scope.
    fn negotiate(&self, bound: BoundClient<'_>, _scope: Option<Scope>) -> Result<PreGrant, RegistrarError> {
        let client = self
            .clients
            .get(bound.client_id.as_ref())
            .expect("Bound client appears to not have been constructed with this registrar");
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
			}).and_then(|client| {
                RegisteredClient::new(client, password_policy).check_authentication(passphrase)
            })?;

        Ok(())
    }
}

