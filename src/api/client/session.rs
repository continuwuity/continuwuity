use std::time::Duration;

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Error, Result, debug, err, info, utils,
	utils::{ReadyExt, hash},
};
use conduwuit_service::uiaa::SESSION_ID_LENGTH;
use futures::StreamExt;
use ruma::{
	UserId,
	api::client::{
		session::{
			get_login_token,
			get_login_types::{
				self,
				v3::{ApplicationServiceLoginType, PasswordLoginType, TokenLoginType},
			},
			login::{
				self,
				v3::{DiscoveryInfo, HomeserverInfo},
			},
			logout, logout_all,
		},
		uiaa,
	},
};

use super::{DEVICE_ID_LENGTH, TOKEN_LENGTH};
use crate::Ruma;

/// # `GET /_matrix/client/v3/login`
///
/// Get the supported login types of this server. One of these should be used as
/// the `type` field when logging in.
#[tracing::instrument(skip_all, fields(%client), name = "login")]
pub(crate) async fn get_login_types_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	_body: Ruma<get_login_types::v3::Request>,
) -> Result<get_login_types::v3::Response> {
	Ok(get_login_types::v3::Response::new(vec![
		get_login_types::v3::LoginType::Password(PasswordLoginType::default()),
		get_login_types::v3::LoginType::ApplicationService(ApplicationServiceLoginType::default()),
		get_login_types::v3::LoginType::Token(TokenLoginType {
			get_login_token: services.server.config.login_via_existing_session,
		}),
	]))
}

/// # `POST /_matrix/client/v3/login`
///
/// Authenticates the user and returns an access token it can use in subsequent
/// requests.
///
/// - The user needs to authenticate using their password (or if enabled using a
///   json web token)
/// - If `device_id` is known: invalidates old access token of that device
/// - If `device_id` is unknown: creates a new device
/// - Returns access token that is associated with the user and device
///
/// Note: You can use [`GET
/// /_matrix/client/r0/login`](fn.get_supported_versions_route.html) to see
/// supported login types.
#[tracing::instrument(skip_all, fields(%client), name = "login")]
pub(crate) async fn login_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<login::v3::Request>,
) -> Result<login::v3::Response> {
	let emergency_mode_enabled = services.config.emergency_password.is_some();

	// Validate login method
	// TODO: Other login methods
	let user_id = match &body.login_info {
		#[allow(deprecated)]
		| login::v3::LoginInfo::Password(login::v3::Password {
			identifier,
			password,
			user,
			..
		}) => {
			debug!("Got password login type");
			let user_id =
				if let Some(uiaa::UserIdentifier::UserIdOrLocalpart(user_id)) = identifier {
					UserId::parse_with_server_name(user_id, &services.config.server_name)
				} else if let Some(user) = user {
					UserId::parse_with_server_name(user, &services.config.server_name)
				} else {
					return Err!(Request(Unknown(
						debug_warn!(?body.login_info, "Valid identifier or username was not provided (invalid or unsupported login type?)")
					)));
				}
				.map_err(|e| err!(Request(InvalidUsername(warn!("Username is invalid: {e}")))))?;

			let lowercased_user_id = UserId::parse_with_server_name(
				user_id.localpart().to_lowercase(),
				&services.config.server_name,
			)?;

			if !services.globals.user_is_local(&user_id)
				|| !services.globals.user_is_local(&lowercased_user_id)
			{
				return Err!(Request(Unknown("User ID does not belong to this homeserver")));
			}

			// first try the username as-is
			let hash = services
				.users
				.password_hash(&user_id)
				.await
				.inspect_err(|e| debug!("{e}"));

			match hash {
				| Ok(hash) => {
					if hash.is_empty() {
						return Err!(Request(UserDeactivated("The user has been deactivated")));
					}

					hash::verify_password(password, &hash)
						.inspect_err(|e| debug!("{e}"))
						.map_err(|_| err!(Request(Forbidden("Wrong username or password."))))?;

					user_id
				},
				| Err(_e) => {
					let hash_lowercased_user_id = services
						.users
						.password_hash(&lowercased_user_id)
						.await
						.inspect_err(|e| debug!("{e}"))
						.map_err(|_| err!(Request(Forbidden("Wrong username or password."))))?;

					if hash_lowercased_user_id.is_empty() {
						return Err!(Request(UserDeactivated("The user has been deactivated")));
					}

					hash::verify_password(password, &hash_lowercased_user_id)
						.inspect_err(|e| debug!("{e}"))
						.map_err(|_| err!(Request(Forbidden("Wrong username or password."))))?;

					lowercased_user_id
				},
			}
		},
		| login::v3::LoginInfo::Token(login::v3::Token { token }) => {
			debug!("Got token login type");
			if !services.server.config.login_via_existing_session {
				return Err!(Request(Unknown("Token login is not enabled.")));
			}
			services.users.find_from_login_token(token).await?
		},
		#[allow(deprecated)]
		| login::v3::LoginInfo::ApplicationService(login::v3::ApplicationService {
			identifier,
			user,
		}) => {
			debug!("Got appservice login type");

			let Some(ref info) = body.appservice_info else {
				return Err!(Request(MissingToken("Missing appservice token.")));
			};

			let user_id =
				if let Some(uiaa::UserIdentifier::UserIdOrLocalpart(user_id)) = identifier {
					UserId::parse_with_server_name(user_id, &services.config.server_name)
				} else if let Some(user) = user {
					UserId::parse_with_server_name(user, &services.config.server_name)
				} else {
					return Err!(Request(Unknown(
						debug_warn!(?body.login_info, "Valid identifier or username was not provided (invalid or unsupported login type?)")
					)));
				}
				.map_err(|e| err!(Request(InvalidUsername(warn!("Username is invalid: {e}")))))?;

			if !services.globals.user_is_local(&user_id) {
				return Err!(Request(Unknown("User ID does not belong to this homeserver")));
			}

			if !info.is_user_match(&user_id) && !emergency_mode_enabled {
				return Err!(Request(Exclusive("Username is not in an appservice namespace.")));
			}

			user_id
		},
		| _ => {
			debug!("/login json_body: {:?}", &body.json_body);
			return Err!(Request(Unknown(
				debug_warn!(?body.login_info, "Invalid or unsupported login type")
			)));
		},
	};

	// Generate new device id if the user didn't specify one
	let device_id = body
		.device_id
		.clone()
		.unwrap_or_else(|| utils::random_string(DEVICE_ID_LENGTH).into());

	// Generate a new token for the device (ensuring no collisions)
	let token = services.users.generate_unique_token().await;

	// Determine if device_id was provided and exists in the db for this user
	let device_exists = if body.device_id.is_some() {
		services
			.users
			.all_device_ids(&user_id)
			.ready_any(|v| v == device_id)
			.await
	} else {
		false
	};

	if device_exists {
		services
			.users
			.set_token(&user_id, &device_id, &token)
			.await?;
	} else {
		services
			.users
			.create_device(
				&user_id,
				&device_id,
				&token,
				body.initial_device_display_name.clone(),
				Some(client.to_string()),
			)
			.await?;
	}

	// send client well-known if specified so the client knows to reconfigure itself
	let client_discovery_info: Option<DiscoveryInfo> = services
		.server
		.config
		.well_known
		.client
		.as_ref()
		.map(|server| DiscoveryInfo::new(HomeserverInfo::new(server.to_string())));

	info!("{user_id} logged in");

	#[allow(deprecated)]
	Ok(login::v3::Response {
		user_id,
		access_token: token,
		device_id,
		well_known: client_discovery_info,
		expires_in: None,
		home_server: Some(services.config.server_name.clone()),
		refresh_token: None,
	})
}

/// # `POST /_matrix/client/v1/login/get_token`
///
/// Allows a logged-in user to get a short-lived token which can be used
/// to log in with the m.login.token flow.
///
/// <https://spec.matrix.org/v1.13/client-server-api/#post_matrixclientv1loginget_token>
#[tracing::instrument(skip_all, fields(%client), name = "login_token")]
pub(crate) async fn login_token_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_login_token::v1::Request>,
) -> Result<get_login_token::v1::Response> {
	if !services.server.config.login_via_existing_session {
		return Err!(Request(Forbidden("Login via an existing session is not enabled")));
	}

	// This route SHOULD have UIA
	// TODO: How do we make only UIA sessions that have not been used before valid?
	let (sender_user, sender_device) = body.sender();

	let mut uiaainfo = uiaa::UiaaInfo {
		flows: vec![uiaa::AuthFlow { stages: vec![uiaa::AuthType::Password] }],
		completed: Vec::new(),
		params: Box::default(),
		session: None,
		auth_error: None,
	};

	match &body.auth {
		| Some(auth) => {
			let (worked, uiaainfo) = services
				.uiaa
				.try_auth(sender_user, sender_device, auth, &uiaainfo)
				.await?;

			if !worked {
				return Err(Error::Uiaa(uiaainfo));
			}

			// Success!
		},
		| _ => match body.json_body.as_ref() {
			| Some(json) => {
				uiaainfo.session = Some(utils::random_string(SESSION_ID_LENGTH));
				services
					.uiaa
					.create(sender_user, sender_device, &uiaainfo, json);

				return Err(Error::Uiaa(uiaainfo));
			},
			| _ => {
				return Err!(Request(NotJson("No JSON body was sent when required.")));
			},
		},
	}

	let login_token = utils::random_string(TOKEN_LENGTH);
	let expires_in = services.users.create_login_token(sender_user, &login_token);

	Ok(get_login_token::v1::Response {
		expires_in: Duration::from_millis(expires_in),
		login_token,
	})
}

/// # `POST /_matrix/client/v3/logout`
///
/// Log out the current device.
///
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
#[tracing::instrument(skip_all, fields(%client), name = "logout")]
pub(crate) async fn logout_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<logout::v3::Request>,
) -> Result<logout::v3::Response> {
	services
		.users
		.remove_device(body.sender_user(), body.sender_device())
		.await;

	Ok(logout::v3::Response::new())
}

/// # `POST /_matrix/client/r0/logout/all`
///
/// Log out all devices of this user.
///
/// - Invalidates all access tokens
/// - Deletes all device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets all to-device events
/// - Triggers device list updates
///
/// Note: This is equivalent to calling [`GET
/// /_matrix/client/r0/logout`](fn.logout_route.html) from each device of this
/// user.
#[tracing::instrument(skip_all, fields(%client), name = "logout")]
pub(crate) async fn logout_all_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<logout_all::v3::Request>,
) -> Result<logout_all::v3::Response> {
	services
		.users
		.all_device_ids(body.sender_user())
		.for_each(|device_id| services.users.remove_device(body.sender_user(), device_id))
		.await;

	Ok(logout_all::v3::Response::new())
}
