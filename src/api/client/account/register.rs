use std::{collections::HashMap, fmt::Write};

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Result, debug_info, error, info,
	utils::{self},
	warn,
};
use conduwuit_service::Services;
use futures::{FutureExt, StreamExt};
use lettre::{Address, message::Mailbox};
use register::RegistrationKind;
use ruma::{
	OwnedUserId, UserId,
	api::client::{
		account::{
			register::{self, LoginType},
			request_registration_token_via_email,
		},
		uiaa::{AuthFlow, AuthType},
	},
	events::{GlobalAccountDataEventType, room::message::RoomMessageEventContent},
	push,
};
use serde_json::value::RawValue;
use service::mailer::messages;

use super::{DEVICE_ID_LENGTH, TOKEN_LENGTH, join_room_by_id_helper};
use crate::Ruma;

const RANDOM_USER_ID_LENGTH: usize = 10;

/// # `POST /_matrix/client/v3/register`
///
/// Register an account on this homeserver.
///
/// You can use [`GET
/// /_matrix/client/v3/register/available`](fn.get_register_available_route.
/// html) to check if the user id is valid and available.
///
/// - Only works if registration is enabled
/// - If type is guest: ignores all parameters except
///   initial_device_display_name
/// - If sender is not appservice: Requires UIAA (but we only use a dummy stage)
/// - If type is not guest and no username is given: Always fails after UIAA
///   check
/// - Creates a new account and populates it with default account data
/// - If `inhibit_login` is false: Creates a device and returns device id and
///   access_token
#[allow(clippy::doc_markdown)]
#[tracing::instrument(skip_all, fields(%client), name = "register", level = "info")]
pub(crate) async fn register_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<register::v3::Request>,
) -> Result<register::v3::Response> {
	let is_guest = body.kind == RegistrationKind::Guest;
	let emergency_mode_enabled = services.config.emergency_password.is_some();

	// Allow registration if it's enabled in the config file or if this is the first
	// run (so the first user account can be created)
	let allow_registration =
		services.config.allow_registration || services.firstrun.is_first_run();

	if !allow_registration && body.appservice_info.is_none() {
		match (body.username.as_ref(), body.initial_device_display_name.as_ref()) {
			| (Some(username), Some(device_display_name)) => {
				info!(
					%is_guest,
					user = %username,
					device_name = %device_display_name,
					"Rejecting registration attempt as registration is disabled"
				);
			},
			| (Some(username), _) => {
				info!(
					%is_guest,
					user = %username,
					"Rejecting registration attempt as registration is disabled"
				);
			},
			| (_, Some(device_display_name)) => {
				info!(
					%is_guest,
					device_name = %device_display_name,
					"Rejecting registration attempt as registration is disabled"
				);
			},
			| (None, _) => {
				info!(
					%is_guest,
					"Rejecting registration attempt as registration is disabled"
				);
			},
		}

		return Err!(Request(Forbidden(
			"This server is not accepting registrations at this time."
		)));
	}

	if is_guest && !services.config.allow_guest_registration {
		info!(
			"Guest registration disabled, rejecting guest registration attempt, initial device \
			 name: \"{}\"",
			body.initial_device_display_name.as_deref().unwrap_or("")
		);
		return Err!(Request(GuestAccessForbidden("Guest registration is disabled.")));
	}

	// forbid guests from registering if there is not a real admin user yet. give
	// generic user error.
	if is_guest && services.firstrun.is_first_run() {
		warn!(
			"Guest account attempted to register before a real admin user has been registered, \
			 rejecting registration. Guest's initial device name: \"{}\"",
			body.initial_device_display_name.as_deref().unwrap_or("")
		);
		return Err!(Request(Forbidden(
			"This server is not accepting registrations at this time."
		)));
	}

	// Appeservices and guests get to skip auth
	let skip_auth = body.appservice_info.is_some() || is_guest;

	let identity = if skip_auth {
		// Appservices and guests have no identity
		None
	} else {
		// Perform UIAA to determine the user's identity
		let (flows, params) = create_registration_uiaa_session(&services).await?;

		Some(
			services
				.uiaa
				.authenticate(&body.auth, flows, params, None)
				.await?,
		)
	};

	// If the user didn't supply a username but did supply an email, use
	// the email's user as their initial localpart to avoid falling back to
	// a randomly generated localpart
	let supplied_username = body.username.clone().or_else(|| {
		if let Some(identity) = &identity
			&& let Some(email) = &identity.email
		{
			Some(email.user().to_owned())
		} else {
			None
		}
	});

	let user_id = determine_registration_user_id(
		&services,
		supplied_username,
		is_guest,
		emergency_mode_enabled,
	)
	.await?;

	if body.body.login_type == Some(LoginType::ApplicationService) {
		// For appservice logins, make sure that the user ID is in the appservice's
		// namespace

		match body.appservice_info {
			| Some(ref info) =>
				if !info.is_user_match(&user_id) && !emergency_mode_enabled {
					return Err!(Request(Exclusive(
						"Username is not in an appservice namespace."
					)));
				},
			| _ => {
				return Err!(Request(MissingToken("Missing appservice token.")));
			},
		}
	} else if services.appservice.is_exclusive_user_id(&user_id).await && !emergency_mode_enabled
	{
		// For non-appservice logins, ban user IDs which are in an appservice's
		// namespace (unless emergency mode is enabled)
		return Err!(Request(Exclusive("Username is reserved by an appservice.")));
	}

	let password = if is_guest { None } else { body.password.as_deref() };

	// Create user
	services.users.create(&user_id, password, None).await?;

	// Set an initial display name
	let mut displayname = user_id.localpart().to_owned();

	// Apply the new user displayname suffix, if it's set
	if !services.globals.new_user_displayname_suffix().is_empty()
		&& body.appservice_info.is_none()
	{
		write!(displayname, " {}", services.server.config.new_user_displayname_suffix)?;
	}

	services
		.users
		.set_displayname(&user_id, Some(displayname.clone()));

	// Initial account data
	services
		.account_data
		.update(
			None,
			&user_id,
			GlobalAccountDataEventType::PushRules.to_string().into(),
			&serde_json::to_value(ruma::events::push_rules::PushRulesEvent {
				content: ruma::events::push_rules::PushRulesEventContent {
					global: push::Ruleset::server_default(&user_id),
				},
			})?,
		)
		.await?;

	// Generate new device id if the user didn't specify one
	let no_device = body.inhibit_login
		|| body
			.appservice_info
			.as_ref()
			.is_some_and(|aps| aps.registration.device_management);

	let (token, device) = if !no_device {
		// Don't create a device for inhibited logins
		let device_id = if is_guest { None } else { body.device_id.clone() }
			.unwrap_or_else(|| utils::random_string(DEVICE_ID_LENGTH).into());

		// Generate new token for the device
		let new_token = utils::random_string(TOKEN_LENGTH);

		// Create device for this account
		services
			.users
			.create_device(
				&user_id,
				&device_id,
				&new_token,
				body.initial_device_display_name.clone(),
				Some(client.to_string()),
			)
			.await?;
		debug_info!(%user_id, %device_id, "User account was created");
		(Some(new_token), Some(device_id))
	} else {
		(None, None)
	};

	// If the user registered with an email, associate it with their account.
	if let Some(identity) = identity
		&& let Some(email) = identity.email
	{
		// This may fail if the email is already in use, but we already check for that
		// in `/requestToken`, so ignoring the error is acceptable here in the rare case
		// that an email is sniped by another user between the `/requestToken` request
		// and the `/register` request.
		let _ = services
			.threepid
			.associate_localpart_email(user_id.localpart(), &email)
			.await;
	}

	let device_display_name = body.initial_device_display_name.as_deref().unwrap_or("");

	// log in conduit admin channel if a non-guest user registered
	if body.appservice_info.is_none() && !is_guest {
		if !device_display_name.is_empty() {
			let notice = format!(
				"New user \"{user_id}\" registered on this server from IP {client} and device \
				 display name \"{device_display_name}\""
			);

			info!("{notice}");
			if services.server.config.admin_room_notices {
				services.admin.notice(&notice).await;
			}
		} else {
			let notice = format!("New user \"{user_id}\" registered on this server.");

			info!("{notice}");
			if services.server.config.admin_room_notices {
				services.admin.notice(&notice).await;
			}
		}
	}

	// log in conduit admin channel if a guest registered
	if body.appservice_info.is_none() && is_guest && services.config.log_guest_registrations {
		debug_info!("New guest user \"{user_id}\" registered on this server.");

		if !device_display_name.is_empty() {
			if services.server.config.admin_room_notices {
				services
					.admin
					.notice(&format!(
						"Guest user \"{user_id}\" with device display name \
						 \"{device_display_name}\" registered on this server from IP {client}"
					))
					.await;
			}
		} else {
			#[allow(clippy::collapsible_else_if)]
			if services.server.config.admin_room_notices {
				services
					.admin
					.notice(&format!(
						"Guest user \"{user_id}\" with no device display name registered on \
						 this server from IP {client}",
					))
					.await;
			}
		}
	}

	if !is_guest {
		// Make the first user to register an administrator and disable first-run mode.
		let was_first_user = services.firstrun.empower_first_user(&user_id).await?;

		// If the registering user was not the first and we're suspending users on
		// register, suspend them.
		if !was_first_user && services.config.suspend_on_register {
			// Note that we can still do auto joins for suspended users
			services
				.users
				.suspend_account(&user_id, &services.globals.server_user)
				.await;
			// And send an @room notice to the admin room, to prompt admins to review the
			// new user and ideally unsuspend them if deemed appropriate.
			if services.server.config.admin_room_notices {
				services
					.admin
					.send_loud_message(RoomMessageEventContent::text_plain(format!(
						"User {user_id} has been suspended as they are not the first user on \
						 this server. Please review and unsuspend them if appropriate."
					)))
					.await
					.ok();
			}
		}
	}

	if body.appservice_info.is_none()
		&& !services.server.config.auto_join_rooms.is_empty()
		&& (services.config.allow_guests_auto_join_rooms || !is_guest)
	{
		for room in &services.server.config.auto_join_rooms {
			let Ok(room_id) = services.rooms.alias.resolve(room).await else {
				error!(
					"Failed to resolve room alias to room ID when attempting to auto join \
					 {room}, skipping"
				);
				continue;
			};

			if !services
				.rooms
				.state_cache
				.server_in_room(services.globals.server_name(), &room_id)
				.await
			{
				warn!(
					"Skipping room {room} to automatically join as we have never joined before."
				);
				continue;
			}

			if let Some(room_server_name) = room.server_name() {
				match join_room_by_id_helper(
					&services,
					&user_id,
					&room_id,
					Some("Automatically joining this room upon registration".to_owned()),
					&[services.globals.server_name().to_owned(), room_server_name.to_owned()],
					&body.appservice_info,
				)
				.boxed()
				.await
				{
					| Err(e) => {
						// don't return this error so we don't fail registrations
						error!(
							"Failed to automatically join room {room} for user {user_id}: {e}"
						);
					},
					| _ => {
						info!("Automatically joined room {room} for user {user_id}");
					},
				}
			}
		}
	}

	Ok(register::v3::Response {
		access_token: token,
		user_id,
		device_id: device,
		refresh_token: None,
		expires_in: None,
	})
}

/// Determine which flows and parameters should be presented when
/// registering a new account.
async fn create_registration_uiaa_session(
	services: &Services,
) -> Result<(Vec<AuthFlow>, Box<RawValue>)> {
	let mut params = HashMap::<String, serde_json::Value>::new();

	let flows = if services.firstrun.is_first_run() {
		// Registration token forced while in first-run mode
		vec![AuthFlow::new(vec![AuthType::RegistrationToken])]
	} else {
		let mut flows = vec![];

		if services
			.registration_tokens
			.iterate_tokens()
			.next()
			.await
			.is_some()
		{
			// Trusted registration flow with a token is available
			let mut token_flow = AuthFlow::new(vec![AuthType::RegistrationToken]);

			if let Some(smtp) = &services.config.smtp
				&& smtp.require_email_for_token_registration
			{
				// Email is required for token registrations
				token_flow.stages.push(AuthType::EmailIdentity);
			}

			flows.push(token_flow);
		}

		let mut untrusted_flow = AuthFlow::default();

		if services.config.recaptcha_private_site_key.is_some() {
			if let Some(pubkey) = &services.config.recaptcha_site_key {
				// ReCaptcha is configured for untrusted registrations
				untrusted_flow.stages.push(AuthType::ReCaptcha);

				params.insert(
					AuthType::ReCaptcha.as_str().to_owned(),
					serde_json::json!({
						"public_key": pubkey,
					}),
				);
			}
		}

		if let Some(smtp) = &services.config.smtp
			&& smtp.require_email_for_registration
		{
			// Email is required for untrusted registrations
			untrusted_flow.stages.push(AuthType::EmailIdentity);
		}

		if !untrusted_flow.stages.is_empty() {
			flows.push(untrusted_flow);
		}

		if flows.is_empty() {
			// No flows are configured. Bail out by default
			// unless open registration was explicitly enabled.
			if !services
				.config
				.yes_i_am_very_very_sure_i_want_an_open_registration_server_prone_to_abuse
			{
				return Err!(Request(Forbidden(
					"This server is not accepting registrations at this time."
				)));
			}

			// We have open registration enabled (😧), provide a dummy flow
			flows.push(AuthFlow::new(vec![AuthType::Dummy]));
		}

		flows
	};

	let params = serde_json::value::to_raw_value(&params).expect("params should be valid JSON");

	Ok((flows, params))
}

async fn determine_registration_user_id(
	services: &Services,
	supplied_username: Option<String>,
	is_guest: bool,
	emergency_mode_enabled: bool,
) -> Result<OwnedUserId> {
	if let Some(supplied_username) = supplied_username
		&& !is_guest
	{
		// The user gets to pick their username. Do some validation to make sure it's
		// acceptable.

		// Don't allow registration with forbidden usernames.
		if services
			.globals
			.forbidden_usernames()
			.is_match(&supplied_username)
			&& !emergency_mode_enabled
		{
			return Err!(Request(Forbidden("Username is forbidden")));
		}

		// Create and validate the user ID
		let user_id = match UserId::parse_with_server_name(
			&supplied_username,
			services.globals.server_name(),
		) {
			| Ok(user_id) => {
				if let Err(e) = user_id.validate_strict() {
					// Unless we are in emergency mode, we should follow synapse's behaviour on
					// not allowing things like spaces and UTF-8 characters in usernames
					if !emergency_mode_enabled {
						return Err!(Request(InvalidUsername(debug_warn!(
							"Username {supplied_username} contains disallowed characters or \
							 spaces: {e}"
						))));
					}
				}

				// Don't allow registration with user IDs that aren't local
				if !services.globals.user_is_local(&user_id) {
					return Err!(Request(InvalidUsername(
						"Username {supplied_username} is not local to this server"
					)));
				}

				user_id
			},
			| Err(e) => {
				return Err!(Request(InvalidUsername(debug_warn!(
					"Username {supplied_username} is not valid: {e}"
				))));
			},
		};

		if services.users.exists(&user_id).await {
			return Err!(Request(UserInUse("User ID is not available.")));
		}

		Ok(user_id)
	} else {
		// The user is a guest or didn't specify a username. Generate a username for
		// them.

		loop {
			let user_id = UserId::parse_with_server_name(
				utils::random_string(RANDOM_USER_ID_LENGTH).to_lowercase(),
				services.globals.server_name(),
			)
			.unwrap();

			if !services.users.exists(&user_id).await {
				break Ok(user_id);
			}
		}
	}
}

/// # `POST /_matrix/client/v3/register/email/requestToken`
///
/// Requests a validation email for the purpose of registering a new account.
pub(crate) async fn request_registration_token_via_email_route(
	State(services): State<crate::State>,
	body: Ruma<request_registration_token_via_email::v3::Request>,
) -> Result<request_registration_token_via_email::v3::Response> {
	let Ok(email) = Address::try_from(body.email.clone()) else {
		return Err!(Request(InvalidParam("Invalid email address.")));
	};

	if services
		.threepid
		.get_localpart_for_email(&email)
		.await
		.is_some()
	{
		return Err!(Request(ThreepidInUse("This email address is already in use.")));
	}

	let session = services
		.threepid
		.send_validation_email(
			Mailbox::new(None, email),
			|verification_link| messages::NewAccount {
				server_name: services.config.server_name.as_ref(),
				verification_link,
			},
			&body.client_secret,
			body.send_attempt.try_into().unwrap(),
		)
		.await?;

	Ok(request_registration_token_via_email::v3::Response::new(session))
}
