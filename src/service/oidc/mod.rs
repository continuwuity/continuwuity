use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use async_trait::async_trait;
use conduwuit::{
	Result,
	config::{OidcConfig, OidcProfileKeyImportMode},
	debug, err, error, info, warn,
};
use database::{Deserialized, Map};
use lettre::Address;
use openidconnect::{
	AdditionalClaims, AuthorizationCode, CsrfToken, EmptyExtraTokenFields, EndpointMaybeSet,
	EndpointNotSet, EndpointSet, IdTokenClaims, IdTokenFields, IssuerUrl, Nonce,
	PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, StandardErrorResponse,
	StandardTokenResponse, TokenResponse,
	core::{
		CoreAuthDisplay, CoreAuthPrompt, CoreAuthenticationFlow, CoreErrorResponseType,
		CoreGenderClaim, CoreJsonWebKey, CoreJweContentEncryptionAlgorithm,
		CoreJwsSigningAlgorithm, CoreProviderMetadata, CoreRevocableToken,
		CoreRevocationErrorResponse, CoreTokenIntrospectionResponse, CoreTokenType,
	},
	reqwest,
};
use ruma::{
	OwnedUserId, UserId,
	api::client::profile::PropagateTo,
	profile::{ProfileFieldName, ProfileFieldValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{runtime, sync::SetOnce};
use url::Url;

use crate::{
	Dep, config, globals, media,
	oauth::grant::AuthorizationCodeResponse,
	threepid,
	users::{self, AccountStatus, ProfileFieldChange},
};

pub struct Service {
	services: Services,
	runtime: runtime::Handle,
	db: Data,
	client: Option<OidcClient>,
}

struct Data {
	openidsubject_localpart: Arc<Map>,
	openidsubject_currentpictureurl: Arc<Map>,
}
struct Services {
	config: Dep<config::Service>,
	globals: Dep<globals::Service>,
	media: Dep<media::Service>,
	threepid: Dep<threepid::Service>,
	users: Dep<users::Service>,
}

struct OidcClient {
	config: OidcConfig,
	machine: SetOnce<OidcClientMachine>,
	client: reqwest::Client,
}

type OidcClientMachine = openidconnect::Client<
	AllClaims,
	CoreAuthDisplay,
	CoreGenderClaim,
	CoreJweContentEncryptionAlgorithm,
	CoreJsonWebKey,
	CoreAuthPrompt,
	StandardErrorResponse<CoreErrorResponseType>,
	StandardTokenResponse<
		IdTokenFields<
			AllClaims,
			EmptyExtraTokenFields,
			CoreGenderClaim,
			CoreJweContentEncryptionAlgorithm,
			CoreJwsSigningAlgorithm,
		>,
		CoreTokenType,
	>,
	CoreTokenIntrospectionResponse,
	CoreRevocableToken,
	CoreRevocationErrorResponse,
	EndpointSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointNotSet,
	EndpointMaybeSet,
	EndpointMaybeSet,
>;

pub type Claims = IdTokenClaims<AllClaims, CoreGenderClaim>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AllClaims {
	#[serde(flatten)]
	pub claims: HashMap<String, Value>,
}

impl AdditionalClaims for AllClaims {}

#[derive(Debug, Deserialize, Serialize)]
pub struct PendingSession {
	pkce_verifier: PkceCodeVerifier,
	nonce: Nonce,
	csrf_token: CsrfToken,
}

pub enum SessionCompletionStatus {
	NeedsUserId,
	Complete(OwnedUserId),
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				config: args.depend::<config::Service>("config"),
                globals: args.depend::<globals::Service>("globals"),
				media: args.depend::<media::Service>("media"),
                threepid: args.depend::<threepid::Service>("threepid"),
                users: args.depend::<users::Service>("users"),
			},
			runtime: args.server.runtime().clone(),
            db: Data {
                openidsubject_localpart: args.db["openidsubject_localpart"].clone(),
				openidsubject_currentpictureurl: args.db["openidsubject_currentpictureurl"].clone(),
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
					OidcClientMachine::from_provider_metadata(
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

	pub fn restricted_profile_fields(&self) -> Vec<ProfileFieldName> {
		if let Some(config) = self.client.as_ref().map(|client| &client.config)
			&& matches!(config.profile_key_import_mode, OidcProfileKeyImportMode::OnLogin)
		{
			config
				.profile_key_map
				.keys()
				.map(|key| ProfileFieldName::from(key.as_str()))
				.collect()
		} else {
			vec![]
		}
	}

	pub async fn begin_session(&self, prompt: Option<CoreAuthPrompt>) -> (PendingSession, Url) {
		let OidcClient { machine, config, .. } =
			self.client.as_ref().expect("oidc should be configured");
		let machine = machine.wait().await;

		let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

		let mut auth_url = machine
			.authorize_url(
				CoreAuthenticationFlow::AuthorizationCode,
				CsrfToken::new_random,
				Nonce::new_random,
			)
			.add_scopes(config.additional_scopes.iter().cloned())
			.set_pkce_challenge(pkce_challenge);

		if let Some(prompt) = prompt {
			auth_url = auth_url.add_prompt(prompt);
		}

		let (auth_url, csrf_token, nonce) = auth_url.url();

		(PendingSession { pkce_verifier, nonce, csrf_token }, auth_url)
	}

	pub async fn exchange_code(
		&self,
		session: PendingSession,
		response: AuthorizationCodeResponse,
	) -> Result<Claims, &'static str> {
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

	#[tracing::instrument(skip(self, claims), fields(subject = claims.subject().to_string()))]
	pub async fn complete_session(
		&self,
		claims: &Claims,
		supplied_user_id: Option<OwnedUserId>,
	) -> Result<SessionCompletionStatus, &'static str> {
		let Some(OidcClient { config, .. }) = self.client.as_ref() else {
			return Err("Delegated authentication is not enabled on this server.");
		};

		// this is a truly awful hack but we really need all the claims in a map
		let all_claims = serde_json::to_value(claims)
			.expect("should be able to serialize claims")
			.as_object()
			.expect("claims should be an object")
			.to_owned();

		debug!(?all_claims);

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
			if let Some(supplied_user_id) = supplied_user_id {
				supplied_user_id
			} else {
				return Ok(SessionCompletionStatus::NeedsUserId);
			}
		} else if let Some(preferred_username) = all_claims
			.get(&config.preferred_username_claim)
			.and_then(|claim| claim.as_str())
		{
			self.services
				.users
				.determine_registration_user_id(Some(preferred_username.to_owned()), None, None)
				.await
				.map_err(|err| {
					error!("Preferred username claim is not a valid localpart: {err}");
					"Your preferred username is not a valid Matrix user ID localpart. Contact \
					 your homeserver's administrator."
				})?
		} else {
			error!("Preferred username claim was not present or was not a string");
			return Err(Self::SERVER_MISCONFIGURED);
		};

		info!(?subject, ?user_id, "User {user_id} successfully authorized with OIDC");

		// Create a shadow account for the user if necessary
		let new_account_registered = match self.services.users.status(&user_id).await {
			| AccountStatus::Active => {
				// Do nothing, an account already exists
				false
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

				info!(?subject, ?user_id, "Shadow user created for {user_id}");
				true
			},
			| AccountStatus::Deactivated => {
				return Err("Your account has been deactivated.");
			},
		};

		self.link_user(&user_id, subject);

		// Import profile fields
		if matches!(config.profile_key_import_mode, OidcProfileKeyImportMode::OnLogin)
			|| (matches!(
				config.profile_key_import_mode,
				OidcProfileKeyImportMode::OnRegistration
			) && new_account_registered)
		{
			if let Some(email_claim) = &config.email_claim {
				if let Some(email) = claims.email().map(|email| email.as_str())
					&& let Ok(address) = Address::from_str(email)
				{
					if let Err(err) = self
						.services
						.threepid
						.associate_localpart_email(user_id.localpart(), &address)
						.await
					{
						warn!(?email_claim, ?address, "Failed to associate email address: {err}");
					}
				} else {
					warn!(
						?email_claim,
						"Email claim was not present or was not a valid email address"
					);
				}
			}

			let user_id = user_id.clone();
			let subject = claims.subject().to_string();
			let profile_key_map = config.profile_key_map.clone();
			let openidsubject_currentpictureurl = self.db.openidsubject_currentpictureurl.clone();
			let users = self.services.users.clone();
			let media = self.services.media.clone();

			let import_task = self.runtime.spawn(async move {
				for (field, claim) in &profile_key_map {
					let Some(value) = all_claims.get(claim).cloned() else {
						warn!(?field, ?claim, "IDP provided no value for this mapped claim");
						continue;
					};

					let value = if let Some(picture_url) = value.as_str()
						&& field == ProfileFieldName::AvatarUrl.as_str()
						&& openidsubject_currentpictureurl
							.get(&subject)
							.await
							.deserialized::<String>()
							.ok()
							.is_none_or(|current_picture| current_picture != picture_url)
					{
						match media.download_media(picture_url).await {
							| Ok((mxc, size)) => {
								openidsubject_currentpictureurl.insert(&subject, picture_url);
								info!(?picture_url, ?mxc, ?size, "Downloaded profile picture");

								ProfileFieldValue::AvatarUrl(mxc)
							},
							| Err(err) => {
								warn!(
									?claim,
									?picture_url,
									"Failed to download profile picture: {err}"
								);
								continue;
							},
						}
					} else {
						match ProfileFieldValue::new(field, value.clone()) {
							| Ok(value) => value,
							| Err(err) => {
								warn!(
									?field,
									?claim,
									?value,
									"Failed to parse claim value for profile field: {err}"
								);
								continue;
							},
						}
					};

					if let Err(err) = users
						.set_profile_field(
							&user_id,
							ProfileFieldChange::Set(value),
							PropagateTo::Unchanged,
						)
						.await
					{
						warn!(?field, ?claim, "Error while setting profile field: {err}");
					}
				}

				info!("Profile import complete");
			});

			// Only wait for import to complete if this is a new account,
			// so they see the correct profile information in the account panel
			if new_account_registered {
				let _ = import_task.await;
			}
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
