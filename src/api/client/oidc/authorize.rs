use oxide_auth_axum::{OAuthResponse, OAuthRequest};
use oxide_auth::{
	endpoint::{OwnerConsent, Solicitation},
	frontends::simple::endpoint::FnSolicitor,
	primitives::registrar::PreGrant,
};
use axum::extract::{Query, State};
use serde_html_form;
use conduwuit::{Result, err};
use reqwest::Url;
use percent_encoding::percent_decode_str;


/// The set of query parameters a client needs to get authorization.
#[derive(serde::Deserialize, Debug)]
pub(crate) struct OAuthQuery {
	client_id: String,
	redirect_uri: Url,
	scope: String,
	state: String,
	code_challenge: String,
	code_challenge_method: String,
	response_type: String,
	response_mode: String,
}

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
	Query(query): Query<OAuthQuery>,
	oauth: OAuthRequest,
) -> Result<OAuthResponse> {
	tracing::trace!("processing OAuth request: {query:?}");
	// Enforce MSC2964's restrictions on OAuth2 flow.
	let Ok(scope) = percent_decode_str(&query.scope).decode_utf8() else {
		return Err(err!(Request(Unknown("the scope could not be percent-decoded"))));
	} ;
	//if ! scope.contains("urn:matrix:api:*") {
	if ! scope.contains("urn:matrix:org.matrix.msc2967.client:api:*") {
		return Err(err!(Request(Unknown("the scope does not include the client API"))));
	}
	if ! scope.contains("urn:matrix:org.matrix.msc2967.client:device:") {
		return Err(err!(Request(Unknown("the scope does not include a device ID"))));
	}
	if query.code_challenge_method != "S256" {
		return Err(err!(Request(Unknown("unsupported code challenge method"))));
	}

	// Redirect to the login page if no token or token not known.
	let hostname = services
		.config
		.well_known
		.client
		.as_ref()
		.map(|s| s.domain().expect("well-known client should be a domain"));
	let login_redirect = OAuthResponse::default()
		.body(&login_form(hostname, &query))
		.content_type("text/html")
		.expect("should set Content-Type on OAuth response");
	match oauth.authorization_header() {
		| None => {
			return Ok(login_redirect);
		},
		| Some(token) => if services.users.find_from_token(token).await.is_err() {
			return Ok(login_redirect); 
		}
	}
	// TODO register the device ID ?

	services
		.oidc
		.endpoint()
		.with_solicitor(FnSolicitor(move |_: &mut _, solicitation: Solicitation<'_>|
			OwnerConsent::InProgress(
				OAuthResponse::default()
					.body(&consent_page_html(
						"/_matrix/client/unstable/org.matrix.msc2964/authorize",
						solicitation,
						hostname.unwrap_or("conduwuit"),
					))
					.content_type("text/html")
					.expect("set content type on consent form")
			)
		))
		.authorization_flow()
		.execute(oauth)
		.map_err(|err| err!("authorization failed: {err:?}"))
}

/// Wether a user allows their device to access this homeserver's resources.
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
	oauth: OAuthRequest,
) -> Result<OAuthResponse> {
	let allowed = allow.unwrap_or(false);
	tracing::debug!("processing user's consent: {:?} - {:?}", allowed, oauth);

	services
		.oidc
		.endpoint()
		.with_solicitor(FnSolicitor(move |_: &mut _, solicitation: Solicitation<'_>|
			match allowed {
				| false => OwnerConsent::Denied,
				| true => OwnerConsent::Authorized(solicitation.pre_grant().client_id.clone())
			}
		))
		.authorization_flow()
		.execute(oauth)
		.map_err(|err| err!(Request(Unknown("consent request failed: {err:?}"))))
}

fn login_form(
	hostname: Option<&str>,
	OAuthQuery {
		client_id,
		redirect_uri,
		scope,
		state,
		code_challenge,
		code_challenge_method,
		response_type,
		response_mode,
	}: &OAuthQuery,
) -> String {
	let hostname = hostname.unwrap_or("");

	format!(
		r#"
			<!DOCTYPE html>
			<html>
			<head>
				<meta charset="utf-8">
				<meta name="viewport" content="width=device-width, initial-scale=1.0">
			</head>
			<body>
				<center>
					<h1>{hostname} login</h1>
					<form action="/_matrix/client/unstable/org.matrix.msc2964/login" method="post">
						<input type="text" name="username" placeholder="Username" required>
						<input type="password" name="password" placeholder="Password" required>
						<input type="hidden" name="client_id" value="{client_id}">
						<input type="hidden" name="redirect_uri" value="{redirect_uri}">
						<input type="hidden" name="scope" value="{scope}">
						<input type="hidden" name="state" value="{state}">
						<input type="hidden" name="code_challenge" value="{code_challenge}">
						<input type="hidden" name="code_challenge_method" value="{code_challenge_method}">
						<input type="hidden" name="response_type" value="{response_type}">
						<input type="hidden" name="response_mode" value="{response_mode}">
						<button type="submit">Login</button>
					</form>
				</center>
			</body>
			</html>
		"#
	)
}

pub(crate) fn consent_page_html(
	route: &str,
	solicitation: Solicitation<'_>,
	hostname: &str,
) -> String {
	let state = solicitation.state();
	let grant = solicitation.pre_grant();
	let PreGrant { client_id, redirect_uri, scope } = grant;
	let mut args = vec![
		("response_type", "code"),
		("client_id", client_id.as_str()),
		("redirect_uri", redirect_uri.as_str()),
	];
	if let Some(state) = state {
		args.push(("state", state));
	}
	let args = serde_html_form::to_string(args).unwrap();

	format!(
		r#"
			<html>
				<head>
					<meta charset='utf-8'>
					<meta name='viewport' content='width=device-width, initial-scale=1.0'>
				</head>
				<body>
					<center>
						<h1>{hostname} login</h1>
						'{client_id}' (at {redirect_uri}) is requesting permission for '{scope}'
						<form method="post">
							<input type="submit" value="Accept" formaction="{route}?{args}&allow=true">
							<input type="submit" value="Deny" formaction="{route}?{args}&deny=true">
						</form>
					</center>
				</body>
			</html>
		"#, 
	)
}
