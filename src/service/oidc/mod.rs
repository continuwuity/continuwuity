use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use conduwuit::{Result, config::OidcConfig, err, error, info};
use database::{Deserialized, Map};
use openidconnect::{
	AuthorizationCode, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl,
	Nonce, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, TokenResponse,
	core::{CoreAuthenticationFlow, CoreClient, CoreIdTokenClaims, CoreProviderMetadata},
	reqwest,
};
use ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use tokio::sync::SetOnce;
use url::Url;

use crate::{
	Dep, config, globals,
	oauth::grant::AuthorizationCodeResponse,
	users::{self, AccountStatus},
};

pub struct Service {
	services: Services,
	db: Data,
	client: Option<OidcClient>,
}

struct Data {
	openidsubject_localpart: Arc<Map>,
}
struct Services {
	config: Dep<config::Service>,
	globals: Dep<globals::Service>,
	users: Dep<users::Service>,
}

struct OidcClient {
	config: OidcConfig,
	machine: SetOnce<
		CoreClient<
			EndpointSet,
			EndpointNotSet,
			EndpointNotSet,
			EndpointNotSet,
			EndpointMaybeSet,
			EndpointMaybeSet,
		>,
	>,
	client: reqwest::Client,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PendingSession {
	pkce_verifier: PkceCodeVerifier,
	nonce: Nonce,
	csrf_token: CsrfToken,
}

pub enum SessionCompletionStatus {
	Complete(OwnedUserId),
	NeedsLocalpart,
	InvalidLocalpart(String),
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				config: args.depend::<config::Service>("config"),
                globals: args.depend::<globals::Service>("globals"),
                users: args.depend::<users::Service>("users"),
			},
            db: Data {
                openidsubject_localpart: args.db["openidsubject_localpart"].clone(),
            },
            client: args.server.config.oauth.oidc.as_ref().map(|config| OidcClient {
                    config: config.clone(),
                    machine: SetOnce::new(),
                    // This isn't in the client service because it has to use the `reqwest` shipped by `openidconnect`
                    client: reqwest::ClientBuilder::new()
                        .connect_timeout(Duration::from_secs(args.server.config.request_conn_timeout))
                        .read_timeout(Duration::from_secs(args.server.config.request_timeout))
                        .timeout(Duration::from_secs(args.server.config.request_total_timeout))
                        .pool_idle_timeout(Duration::from_secs(args.server.config.request_idle_timeout))
                        .pool_max_idle_per_host(args.server.config.request_idle_per_host.into())
                        .user_agent(conduwuit::user_agent())
                        .redirect(reqwest::redirect::Policy::none())
                        .danger_accept_invalid_certs(args.server.config.allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure)
                        .build()
                        .expect("client should build")
                }),
		}))
	}

	async fn worker(self: Arc<Self>) -> Result {
		if let Some(OidcClient { config, machine, client }) = &self.client {
			let redirect_url = self
				.services
				.config
				.get_client_domain()
				.join(&format!("{}/oidc/complete", conduwuit::ROUTE_PREFIX))
				.expect("redirect url should be valid");

			let provider_metadata = CoreProviderMetadata::discover_async(
				IssuerUrl::from_url(config.discovery_url.clone()),
				client,
			)
			.await
			.map_err(|err| err!("Failed to discover OIDC provider metadata: {err}"))?;

			machine
				.set(
					CoreClient::from_provider_metadata(
						provider_metadata,
						config.client_id.clone(),
						Some(config.client_secret.clone()),
					)
					.set_redirect_uri(RedirectUrl::from_url(redirect_url)),
				)
				.expect("machine should be empty");
		}

		Ok(())
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	const SERVER_MISCONFIGURED: &str =
		"Identity server is misconfigured. Contact your homeserver's administrator.";

	pub fn enabled(&self) -> bool { self.client.is_some() }

	pub async fn begin_session(&self) -> (PendingSession, Url) {
		let OidcClient { machine, .. } = self.client.as_ref().expect("oidc should be configured");
		let machine = machine.wait().await;

		let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

		let (auth_url, csrf_token, nonce) = machine
			.authorize_url(
				CoreAuthenticationFlow::AuthorizationCode,
				CsrfToken::new_random,
				Nonce::new_random,
			)
			.set_pkce_challenge(pkce_challenge)
			.url();

		(PendingSession { pkce_verifier, nonce, csrf_token }, auth_url)
	}

	pub async fn exchange_code(
		&self,
		session: PendingSession,
		response: AuthorizationCodeResponse,
	) -> Result<CoreIdTokenClaims, &'static str> {
		let Some(OidcClient { machine, client, .. }) = self.client.as_ref() else {
			return Err("Delegated authentication is not enabled on this server.");
		};

		let machine = machine.wait().await;

		if session.csrf_token.into_secret() != response.state {
			return Err("State mismatch.");
		}

		let token_response = machine
			.exchange_code(AuthorizationCode::new(response.code))
			.expect("machine should be configured correctly")
			.set_pkce_verifier(session.pkce_verifier)
			.request_async(client)
			.await
			.map_err(|err| {
				error!("Failed to exchange OIDC authorization code: {err}");
				"Code exchange failed."
			})?;

		let Some(id_token) = token_response.id_token() else {
			error!("Identity server did not return an id token");
			return Err(Self::SERVER_MISCONFIGURED);
		};

		let claims = id_token
			.claims(&machine.id_token_verifier(), &session.nonce)
			.map_err(|err| {
				error!("Failed to verify id token claims: {err}");
				Self::SERVER_MISCONFIGURED
			})?
			.to_owned();

		info!(subject = claims.subject().as_str(), "Authenticated subject");

		Ok(claims)
	}

	pub async fn complete_session(
		&self,
		claims: &CoreIdTokenClaims,
		supplied_username: Option<String>,
	) -> Result<SessionCompletionStatus, &'static str> {
		let Some(OidcClient { config, .. }) = self.client.as_ref() else {
			return Err("Delegated authentication is not enabled on this server.");
		};

		let subject = claims.subject().as_str();

		let user_id = if let Ok(localpart) = self
			.db
			.openidsubject_localpart
			.get(subject)
			.await
			.deserialized::<String>()
		{
			UserId::parse(format!("@{localpart}:{}", self.services.globals.server_name()))
				.expect("saved localpart should be valid")
		} else if config.prompt_for_localpart {
			if let Some(supplied_username) = supplied_username {
				match self
					.services
					.users
					.determine_registration_user_id(Some(supplied_username), None, None)
					.await
				{
					| Ok(user_id) => user_id,
					| Err(err) =>
						return Ok(SessionCompletionStatus::InvalidLocalpart(err.message())),
				}
			} else {
				return Ok(SessionCompletionStatus::NeedsLocalpart);
			}
		} else if let Some(preferred_username) = claims.preferred_username() {
			self.services
				.users
				.determine_registration_user_id(Some(preferred_username.to_string()), None, None)
				.await
				.map_err(|err| {
					error!("Preferred username claim is not a valid localpart: {err}");
					"Your preferred username is not a valid Matrix user ID localpart. Contact \
					 your homeserver's administrator."
				})?
		} else {
			error!("No preferred username claim was present");
			return Err(Self::SERVER_MISCONFIGURED);
		};

		info!(?subject, ?user_id, "User {user_id} successfully authorized with OIDC");

		match self.services.users.status(&user_id).await {
			| AccountStatus::Active => {
				// Do nothing, an account already exists
			},
			| AccountStatus::NotFound => {
				// Create a new shadow user
				self.services
					.users
					.create_local_account(&user_id, None, None)
					.await
					.map_err(|err| {
						error!("Failed to create a shadow user for {user_id}: {err}");
						Self::SERVER_MISCONFIGURED
					})?;

				self.link_user(&user_id, subject);

				info!(?subject, ?user_id, "Shadow user created for {user_id}");
			},
			| AccountStatus::Deactivated => {
				return Err("Your account has been deactivated.");
			},
		}

		Ok(SessionCompletionStatus::Complete(user_id))
	}

	pub fn link_user(&self, user_id: &UserId, subject: &str) {
		self.db
			.openidsubject_localpart
			.insert(subject, user_id.localpart());
	}

	pub fn unlink_user(&self, subject: &str) { self.db.openidsubject_localpart.remove(subject); }
}
