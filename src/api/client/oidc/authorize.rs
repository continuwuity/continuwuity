use axum::extract::{Query, State};
use conduwuit::{Result, err};
use conduwuit_web::oidc::{
	AuthorizationQuery, OidcRequest, OidcResponse, oidc_consent_form, oidc_login_form,
};
use oxide_auth::{
	endpoint::{OwnerConsent, Solicitation},
	frontends::simple::endpoint::FnSolicitor,
};
use percent_encoding::percent_decode_str;

/// # `GET /_matrix/client/unstable/org.matrix.msc2964/authorize`
///
/// Authenticate a user and device, and solicit the user's consent.
///
/// Redirects to the login page if no token or token not belonging to any user.
/// [super::login::oidc_login] takes it up at the same point, so it's either
/// the client has a token, or the user does user password. Then the user gets
/// access to stage two, [authorize_consent].
pub(crate) async fn authorize(
	State(services): State<crate::State>,
	Query(query): Query<AuthorizationQuery>,
	oauth: OidcRequest,
) -> Result<OidcResponse> {
	tracing::trace!("processing OAuth request: {query:?}");
	// Enforce MSC2964's restrictions on OAuth2 flow.
	let Ok(scope) = percent_decode_str(&query.scope).decode_utf8() else {
		return Err(err!(Request(Unknown("the scope could not be percent-decoded"))));
	};
	//if ! scope.contains("urn:matrix:api:*") {
	if !scope.contains("urn:matrix:org.matrix.msc2967.client:api:*") {
		return Err(err!(Request(Unknown("the scope does not include the client API"))));
	}
	if !scope.contains("urn:matrix:org.matrix.msc2967.client:device:") {
		return Err(err!(Request(Unknown("the scope does not include a device ID"))));
	}
	if query.code_challenge_method != "S256" {
		return Err(err!(Request(Unknown("unsupported code challenge method"))));
	}

	// Redirect to the login page if no token or token not known.
	let hostname = services.config.server_name.host();
	match oauth.authorization_header() {
		| None => {
			return Ok(oidc_login_form(hostname, &query));
		},
		| Some(token) =>
			if services.users.find_from_token(token).await.is_err() {
				return Ok(oidc_login_form(hostname, &query));
			},
	}
	// TODO register the device ID ?

	services
		.oidc
		.endpoint()
		.with_solicitor(oidc_consent_form(hostname, &query))
		.authorization_flow()
		.execute(oauth)
		.map_err(|err| err!("authorization failed: {err:?}"))
}

/// Whether a user allows their device to access this homeserver's resources.
#[derive(serde::Deserialize)]
pub(crate) struct Allowance {
	allow: Option<bool>,
}

/// # `POST /_matrix/client/unstable/org.matrix.msc2964/authorize?allow=[Option<bool>]`
///
/// Authorize the device based on the user's consent. If the user allows
/// it to access their data, the client may request a token at the
/// [super::token::token] endpoint.
pub(crate) async fn authorize_consent(
	Query(Allowance { allow }): Query<Allowance>,
	State(services): State<crate::State>,
	oauth: OidcRequest,
) -> Result<OidcResponse> {
	let allowed = allow.unwrap_or(false);
	tracing::debug!("processing user's consent: {:?} - {:?}", allowed, oauth);

	services
		.oidc
		.endpoint()
		.with_solicitor(FnSolicitor(
			move |_: &mut _, solicitation: Solicitation<'_>| match allowed {
				| false => OwnerConsent::Denied,
				| true => OwnerConsent::Authorized(solicitation.pre_grant().client_id.clone()),
			},
		))
		.authorization_flow()
		.execute(oauth)
		.map_err(|err| err!(Request(Unknown("consent request failed: {err:?}"))))
}
