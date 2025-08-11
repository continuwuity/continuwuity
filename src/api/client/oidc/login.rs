use axum::extract::State;
use conduwuit::{Result, err, utils::hash::verify_password};
use conduwuit_web::oidc::{
	LoginError, LoginQuery, OidcRequest, OidcResponse, oidc_consent_form,
};
use ruma::user_id::UserId;

//#[axum::debug_handler]
/// # `POST /_matrix/client/unstable/org.matrix.msc2964/login`
///
/// Display a login UI to the user and return an authorization code on success.
/// We presume that the OAuth2 query parameters are provided in the form.
/// With the code, the client may then access stage two,
/// [super::authorize::authorize_consent].
pub(crate) async fn oidc_login(
	State(services): State<crate::State>,
	request: OidcRequest,
) -> Result<OidcResponse> {
	let query: LoginQuery = request.clone().try_into().map_err(|LoginError(err)| {
		err!(Request(InvalidParam("Cannot process login form. {err}")))
	})?;
	tracing::trace!("processing login query {:#?}", query.clone());
	// Only accept local usernames. Mostly to simplify things at first.
	let user_id =
		UserId::parse_with_server_name(query.username.clone(), &services.config.server_name)
			.map_err(|e| err!(Request(InvalidUsername("Username is invalid: {e}"))))?;

	if !services.users.exists(&user_id).await {
		return Err(err!(Request(Unknown("unknown username"))));
	}
	let valid_hash = services.users.password_hash(&user_id).await?;

	if valid_hash.is_empty() {
		return Err(err!(Request(UserDeactivated("the user's hash was not found"))));
	}
	if verify_password(&query.password, &valid_hash).is_err() {
		return Err(err!(Request(InvalidParam("password does not match"))));
	}

	let hostname = services.config.server_name.host();
	tracing::info!("logging in {user_id:?}");

	services
		.oidc
		.endpoint()
		.with_solicitor(oidc_consent_form(hostname, &query.into()))
		.authorization_flow()
		.execute(request)
		.map_err(|err| err!(Request(Unknown("authorisation failed: {err:?}"))))
}
