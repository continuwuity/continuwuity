use std::time::Duration;

use axum::extract::State;
use axum_client_ip::ClientIp;
use conduwuit::{
	Err, Result, debug, err, info,
	utils::{self, ReadyExt, hash, stream::BroadbandExt},
	warn,
};
use conduwuit_core::debug_error;
use conduwuit_service::Services;
use futures::StreamExt;
use lettre::Address;
use ruma::{
	OwnedUserId, UserId,
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
		uiaa::{EmailUserIdentifier, MatrixUserIdentifier, UserIdentifier},
	},
	assign,
};
use service::uiaa::Identity;

use super::{DEVICE_ID_LENGTH, TOKEN_LENGTH};
use crate::Ruma;

/// # `GET /_matrix/client/v3/login`
///
/// Get the supported login types of this server. One of these should be used as
/// the `type` field when logging in.
#[tracing::instrument(skip_all, fields(%client), name = "login", level = "info")]
pub(crate) async fn get_login_types_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	_body: Ruma<get_login_types::v3::Request>,
) -> Result<get_login_types::v3::Response> {
	Ok(get_login_types::v3::Response::new(vec![
		get_login_types::v3::LoginType::Password(PasswordLoginType::default()),
		get_login_types::v3::LoginType::ApplicationService(ApplicationServiceLoginType::default()),
		get_login_types::v3::LoginType::Token(assign!(TokenLoginType::new(), {
			get_login_token: services.server.config.login_via_existing_session,
		})),
	]))
}

/// Authenticates the given user by its ID and its password.
///
/// Returns the user ID if successful, and an error otherwise.
#[tracing::instrument(skip_all, fields(%user_id), name = "password", level = "debug")]
pub(crate) async fn password_login(
	services: &Services,
	user_id: &UserId,
	lowercased_user_id: &UserId,
	password: &str,
) -> Result<OwnedUserId> {
	let (hash, user_id) = match services.users.password_hash(user_id).await {
		| Ok(hash) => (hash, user_id),
		| Err(_) => services
			.users
			.password_hash(lowercased_user_id)
			.await
			.map(|hash| (hash, lowercased_user_id))
			.map_err(|_| err!(Request(Forbidden("Invalid identifier or password."))))?,
	};

	if hash.is_empty() {
		return Err!(Request(UserDeactivated("The user has been deactivated")));
	}

	hash::verify_password(password, &hash)
		.inspect_err(|e| debug_error!("{e}"))
		.map_err(|_| err!(Request(Forbidden("Invalid identifier or password."))))?;

	Ok(user_id.to_owned())
}

pub(crate) async fn handle_login(
	services: &Services,
	identifier: Option<&UserIdentifier>,
	password: &str,
	user: Option<&String>,
) -> Result<OwnedUserId> {
	debug!("Got password login type");

	let user_id_or_localpart = match (identifier, user) {
		| (Some(UserIdentifier::Matrix(MatrixUserIdentifier { user, .. })), _)
		| (None, Some(user)) => user,
		| (Some(UserIdentifier::Email(EmailUserIdentifier { address, .. })), _) => {
			let email = Address::try_from(address.to_owned())
				.map_err(|_| err!(Request(InvalidParam("Email is malformed"))))?;

			&services
				.threepid
				.get_localpart_for_email(&email)
				.await
				.ok_or_else(|| err!(Request(Forbidden("Invalid identifier or password"))))?
		},
		| _ => {
			return Err!(Request(InvalidParam("Identifier type not recognized")));
		},
	};

	let user_id =
		UserId::parse_with_server_name(user_id_or_localpart, &services.config.server_name)
			.map_err(|_| err!(Request(InvalidUsername("User ID is malformed"))))?;

	let lowercased_user_id = UserId::parse_with_server_name(
		user_id.localpart().to_lowercase(),
		&services.config.server_name,
	)
	.unwrap();

	if !services.globals.user_is_local(&user_id)
		|| !services.globals.user_is_local(&lowercased_user_id)
	{
		return Err!(Request(InvalidParam("User ID does not belong to this homeserver")));
	}

	if services.users.is_locked(&user_id).await? {
		return Err!(Request(UserLocked("This account has been locked.")));
	}

	if services.users.is_login_disabled(&user_id).await {
		warn!(%user_id, "user attempted to log in with a login-disabled account");
		return Err!(Request(Forbidden("This account is not permitted to log in.")));
	}

	password_login(services, &user_id, &lowercased_user_id, password).await
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
#[tracing::instrument(skip_all, fields(%client), name = "login", level = "info")]
pub(crate) async fn login_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
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
		}) => handle_login(&services, identifier.as_ref(), password, user.as_ref()).await?,
		| login::v3::LoginInfo::Token(login::v3::Token { token, .. }) => {
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
			..
		}) => {
			debug!("Got appservice login type");

			let Some(ref info) = body.appservice_info else {
				return Err!(Request(MissingToken("Missing appservice token.")));
			};

			let user_id =
				if let Some(UserIdentifier::Matrix(MatrixUserIdentifier { user, .. })) = identifier {
					UserId::parse_with_server_name(user, &services.config.server_name)
				} else if let Some(user) = user {
					UserId::parse_with_server_name(user, &services.config.server_name)
				} else {
					return Err!(Request(Unknown(
						debug_warn!(?body.login_info, "Valid identifier or username was not provided (invalid or unsupported login type?)")
					)));
				}
				.map_err(|_| err!(Request(InvalidUsername(warn!("User ID is malformed")))))?;

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
	Ok(assign!(login::v3::Response::new(user_id, token, device_id), {
		well_known: client_discovery_info,
		expires_in: None,
		home_server: Some(services.config.server_name.clone()),
		refresh_token: None,
	}))
}

/// # `POST /_matrix/client/v1/login/get_token`
///
/// Allows a logged-in user to get a short-lived token which can be used
/// to log in with the m.login.token flow.
///
/// <https://spec.matrix.org/v1.13/client-server-api/#post_matrixclientv1loginget_token>
#[tracing::instrument(skip_all, fields(%client), name = "login_token", level = "info")]
pub(crate) async fn login_token_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<get_login_token::v1::Request>,
) -> Result<get_login_token::v1::Response> {
	if !services.server.config.login_via_existing_session {
		return Err!(Request(Forbidden("Login via an existing session is not enabled")));
	}

	let sender_user = body.sender_user();

	// Prompt the user to confirm with their password using UIAA
	let _ = services
		.uiaa
		.authenticate_password(&body.auth, Some(Identity::from_user_id(sender_user)))
		.await?;

	let login_token = utils::random_string(TOKEN_LENGTH);
	let expires_in = services.users.create_login_token(sender_user, &login_token);

	Ok(get_login_token::v1::Response::new(
		Duration::from_millis(expires_in),
		login_token,
	))
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
#[tracing::instrument(skip_all, fields(%client), name = "logout", level = "info")]
pub(crate) async fn logout_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<logout::v3::Request>,
) -> Result<logout::v3::Response> {
	let (sender_user, sender_device) = body.sender();
	services
		.users
		.remove_device(sender_user, sender_device)
		.await;
	services
		.pusher
		.get_pushkeys(sender_user)
		.map(ToOwned::to_owned)
		.broad_filter_map(async |pushkey| {
			services
				.pusher
				.get_pusher_device(&pushkey)
				.await
				.ok()
				.as_ref()
				.is_some_and(|pusher_device| pusher_device == sender_device)
				.then_some(pushkey)
		})
		.for_each(async |pushkey| {
			services.pusher.delete_pusher(sender_user, &pushkey).await;
		})
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
#[tracing::instrument(skip_all, fields(%client), name = "logout", level = "info")]
pub(crate) async fn logout_all_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<logout_all::v3::Request>,
) -> Result<logout_all::v3::Response> {
	let sender_user = body.sender_user();
	services
		.users
		.all_device_ids(sender_user)
		.for_each(async |device_id| services.users.remove_device(sender_user, &device_id).await)
		.await;
	services
		.pusher
		.get_pushkeys(sender_user)
		.for_each(async |pushkey| {
			services.pusher.delete_pusher(sender_user, pushkey).await;
		})
		.await;

	Ok(logout_all::v3::Response::new())
}
