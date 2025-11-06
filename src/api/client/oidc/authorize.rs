use axum::extract::{Query, State};
use conduwuit::{Result, err, utils::ReadyExt};
use conduwuit_web::oidc::{
	AuthorizationQuery, OidcRequest, OidcResponse, oidc_consent_form, oidc_login_form,
};
use oxide_auth::{
	endpoint::{OwnerConsent, Solicitation},
	frontends::simple::endpoint::FnSolicitor,
};
use percent_encoding::percent_decode_str;
use ruma::UserId;
use service::oidc::{SCOPE_PREFIX_API, SCOPE_PREFIX_DEVICE};

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
	tracing::trace!("processing OAuth request: {query:#?}");
	// Enforce MSC2964's restrictions on OAuth2 flow.
	let Ok(scope) = percent_decode_str(&query.scope).decode_utf8() else {
		return Err(err!(Request(Unknown("the scope could not be percent-decoded"))));
	};
	if !scope.contains(&format!("{SCOPE_PREFIX_API}*")) {
		return Err(err!(Request(Unknown("the scope does not include the client API"))));
	}
	if !scope.contains(SCOPE_PREFIX_DEVICE) {
		return Err(err!(Request(Unknown("the scope does not include a device ID"))));
	}
	if query.code_challenge_method != "S256" {
		return Err(err!(Request(Unknown("unsupported code challenge method"))));
	}

	// Redirect to the login page if no token or token not known.
	let hostname = services.config.server_name.host();
	let Some(token) = oauth.authorization_header() else {
		return Ok(oidc_login_form(hostname, &query));
	};

	tracing::debug!("submitting OIDC authorisation for token : {token:#?}");
	// Get the user id from the token and add it to the query.
	let (owner_id, _) = services.oidc.user_and_device_from_token(token).await?;
	let mut query_with_user_id = query.clone();
	query_with_user_id.username = Some(owner_id.localpart().to_owned());

	services
		.oidc
		.endpoint()
		.with_solicitor(oidc_consent_form(hostname, &query_with_user_id))
		.authorization_flow()
		.execute(oauth)
		.map_err(|err| err!("authorization failed: {err:?}"))
}

/// Whether a user allows their device to access this homeserver's resources.
#[derive(serde::Deserialize)]
pub(crate) struct Allowance {
	allow: Option<String>,
}

/// # `POST /_matrix/client/unstable/org.matrix.msc2964/authorize?allow=[Option<String>]`
///
/// Authorize the device based on the owner's consent. If the owner allows
/// it to access their data, the client may request a token at the
/// [super::token::token] endpoint.
///
/// On the owner's consent, if their specific device is unregistered it will be
/// registered in their device list (not to be confused with the OIDC client
/// registration).
pub(crate) async fn authorize_consent(
	Query(Allowance { allow }): Query<Allowance>,
	State(services): State<crate::State>,
	Query(query): Query<AuthorizationQuery>,
	oauth: OidcRequest,
) -> Result<OidcResponse> {
	tracing::debug!("processing owner's consent: {:?}", allow);
	tracing::trace!("owner's consent request: {:#?}", query);
	let Some(owner_id) = allow.clone() else {
		return Err(err!(Request(Unknown("the owner did not consent to the client's access"))));
	};
	let server_name = services.globals.server_name();
	let owner_id = UserId::parse_with_server_name(owner_id.clone(), server_name)
		.map_err(|err| err!(Request(InvalidUsername("invalid username {owner_id:?}: {err}"))))?;
	let Some(matrix_client) = services
		.oidc
		.client_from_client_id(&query.client_id)
		.await?
	else {
		return Err(err!(Request(Unknown(
			"no client has registered client_id {:?}",
			query.client_id
		))));
	};
	let scope = query.scope.parse().map_err(|err| {
		err!(Request(Unknown("could not parse scope {:?}: {}", query.scope, err)))
	})?;
	let device_id = services.oidc.device_id_from_scope(&scope)?;
	// Check that the device is registered in the owner devices list.
	// Note that this is _not_ the OIDC client registration.
	let device_is_registered_with_owner = services
		.users
		.all_device_ids(&owner_id)
		.ready_any(|v| v == device_id)
		.await;
	if !device_is_registered_with_owner {
		// TODO get the client's IP from the request.
		let client_ip = None;
		services
			.oidc
			.register_device(
				&query.client_id,
				(&owner_id, &device_id),
				matrix_client.name.as_deref(),
				client_ip,
			)
			.await?;
	}

	services
		.oidc
		.endpoint()
		.with_solicitor(FnSolicitor(move |_: &mut _, _: Solicitation<'_>| match allow.clone() {
			| None => OwnerConsent::Denied,
			| Some(user_id) => OwnerConsent::Authorized(user_id),
		}))
		.authorization_flow()
		.execute(oauth)
		.map_err(|err| err!(Request(Unknown("consent request failed: {err:?}"))))
}
