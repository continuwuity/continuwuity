use super::authorize::consent_page_html;
use oxide_auth_axum::{OAuthRequest, OAuthResponse};
use oxide_auth::{
	endpoint::{OwnerConsent, Solicitation},
	frontends::simple::endpoint::FnSolicitor,
};
use axum::extract::{Form, FromRequest, State};
use conduwuit::{
	Result,
	err,
	utils::hash::verify_password,
};
use ruma::user_id::UserId;
use reqwest::Url;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

/// The set of query parameters a client needs to get authorization.
#[derive(serde::Deserialize, Debug)]
pub(crate) struct LoginForm {
	username: String,
	password: String,
	client_id: String,
	redirect_uri: Url,
	scope: String,
	state: String,
	code_challenge: String,
	code_challenge_method: String,
	response_type: String,
	response_mode: String,
}

impl From<LoginForm> for String {
	/// Turn the OAuth parameters from a Form into a GET query, suitable for
	/// then turning it into oxide-auth's OAuthRequest. Strips the unneeded
	/// username and password.
	fn from(value: LoginForm) -> Self {
		let encode = |text: &str| -> String {
			utf8_percent_encode(text, NON_ALPHANUMERIC).to_string()
		};

		format!(
			"?client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method={}&response_type={}&response_mode={}",
			encode(&value.client_id),
			encode(&value.redirect_uri.to_string()),
			encode(&value.scope),
			encode(&value.state),
			encode(&value.code_challenge),
			encode(&value.code_challenge_method),
			encode(&value.response_type),
			encode(&value.response_mode),
		)
	}
}

/// # `POST /_matrix/client/unstable/org.matrix.msc2964/login`
///
/// Display a login UI to the user and return an authorization code on success.
/// We presume that the OAuth2 query parameters are provided in the form.
/// With the code, the client may then access stage two,
/// [super::authorize::authorize_consent].
pub(crate) async fn oidc_login(
	State(services): State<crate::State>,
	Form(login_query): Form<LoginForm>,
) -> Result<OAuthResponse> {
	// Only accept local usernames. Mostly to simplify things at first.
	let user_id = UserId::parse_with_server_name(
			login_query.username.clone(),
			&services.config.server_name
		)
		.map_err(|e| err!(Request(InvalidUsername(warn!("Username is invalid: {e}")))))?;

	if ! services.users.exists(&user_id).await {
		return Err(err!(Request(Unknown("unknown username"))));
	}
	tracing::info!("logging in: {user_id:?}");
	let valid_hash = services
		.users
		.password_hash(&user_id)
		.await
		.inspect_err(|e| tracing::info!("could not get user's hash: {e:?}"))?;

	if valid_hash.is_empty() {
		return Err(err!(Request(UserDeactivated("the user's hash was not found"))));
	}
	if let Err(_) = verify_password(&login_query.password, &valid_hash) {
		return Err(err!(Request(InvalidParam("password does not match"))));
	}

	// Build up a GET query and parse it as an OAuthRequest.
	let login_query: String = login_query.into();
	let login_url = http::Uri::builder()
		.scheme("https")
		.authority(services.config.server_name.as_str())
		.path_and_query(login_query)
		.build()
		.expect("should be parseable");
	let req: http::Request<axum::body::Body> = http::Request::builder()
		.method("GET")
		.uri(&login_url)
		.body(axum::body::Body::empty())
		.expect("login form OAuth parameters parseable as a query");
	let oauth = OAuthRequest::from_request(req, &"")
		.await
		.expect("request parseable as an OAuth query");
	let hostname = services
		.config
		.well_known
		.client
		.as_ref()
		.map(|s| s.domain().expect("well-known client should be a domain"));

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
		.map_err(|err| err!(Request(Unknown("authorization failed: {err:?}"))))
}
