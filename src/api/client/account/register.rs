use std::collections::HashMap;

use axum::extract::State;
use conduwuit::{
	Err, Result, debug_info, info,
	utils::{self},
};
use conduwuit_service::Services;
use futures::StreamExt;
use lettre::{Address, message::Mailbox};
use ruma::{
	api::client::{
		account::{
			register::{self, LoginType, RegistrationKind},
			request_registration_token_via_email,
		},
		uiaa::{AuthFlow, AuthType},
	},
	assign,
};
use serde_json::value::RawValue;
use service::{
	mailer::messages,
	users::{DeviceToken, HashedPassword},
};

use super::DEVICE_ID_LENGTH;
use crate::{Ruma, client_ip::ClientIp};

/// # `POST /_matrix/client/v3/register`
///
/// Register an account on this homeserver.
///
/// You can use [`GET
/// /_matrix/client/v3/register/available`](fn.get_register_available_route.
/// html) to check if the user id is valid and available.
#[allow(clippy::doc_markdown)]
#[tracing::instrument(skip_all, fields(%client), name = "register", level = "info")]
pub(crate) async fn register_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<register::v3::Request>,
) -> Result<register::v3::Response> {
	if body.kind != RegistrationKind::User {
		return Err!(Request(GuestAccessForbidden("Guests may not register on this server.")));
	}

	// Allow registration if it's enabled in the config file or if this is the first
	// run (so the first user account can be created)
	let allow_registration =
		services.config.allow_registration || services.firstrun.is_first_run();

	if !allow_registration && body.identity.is_none() {
		info!(
			?body.username,
			?body.initial_device_display_name,
			"Rejecting registration attempt as registration is disabled"
		);

		return Err!(Request(Forbidden(
			"This server is not accepting registrations at this time."
		)));
	}

	let user_id = if body.body.login_type == Some(LoginType::ApplicationService) {
		let Some(appservice_info) = &body.identity else {
			return Err!(Request(Forbidden(
				"Only appservices can use the appservice login type."
			)));
		};

		let user_id = services
			.users
			.determine_registration_user_id(body.username.clone(), None, Some(appservice_info))
			.await?;

		services.users.create_shadow_account(&user_id).await?;

		user_id
	} else {
		// Perform UIAA to determine the user's identity
		let (flows, params) = create_registration_uiaa_session(&services).await?;

		let identity = services
			.uiaa
			.authenticate(&body.auth, flows, params, None)
			.await?;

		let password = if let Some(password) = &body.password {
			HashedPassword::new(password)?
		} else {
			return Err!(Request(InvalidParam("A password must be provided.")));
		};

		let user_id = services
			.users
			.determine_registration_user_id(body.username.clone(), identity.email.as_ref(), None)
			.await?;

		services
			.users
			.create_local_account(&user_id, Some(password), identity.email)
			.await?;

		user_id
	};

	let (token, device) = if !body.inhibit_login {
		// If UIAA is disabled, we can't create a device. In that case only appservices
		// can reach this point in the first place, so we return an error for them.
		if !services.config.oauth.compatibility_mode().uiaa_available() {
			return Err!(Request(AppserviceLoginUnsupported(
				"User-interactive appservice registration is not available on this server."
			)));
		}

		// Generate new device id if the user didn't specify one
		let device_id = body
			.device_id
			.clone()
			.unwrap_or_else(|| utils::random_string(DEVICE_ID_LENGTH).into());

		// Generate new token for the device
		let new_token = DeviceToken::new_random();

		// Create device for this account
		services
			.users
			.create_device(
				&user_id,
				&device_id,
				Some(new_token.clone()),
				body.initial_device_display_name.clone(),
				Some(client.to_string()),
			)
			.await?;
		(Some(new_token), Some(device_id))
	} else {
		// Don't create a device for inhibited logins
		(None, None)
	};

	debug_info!(%user_id, ?device, "New account created via legacy registration");

	Ok(assign!(register::v3::Response::new(user_id), {
		access_token: token.map(DeviceToken::into_token),
		device_id: device,
		refresh_token: None,
		expires_in: None,
	}))
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

		// Require all users to agree to the terms and conditions, if configured
		let terms = &services.config.registration_terms;
		if !terms.documents.is_empty() {
			let mut terms_map = HashMap::new();

			for (id, document) in &terms.documents {
				terms_map.insert(id.to_owned(), serde_json::json!({
					terms.language.clone(): serde_json::to_value(document).expect("should be able to serialize document")
				}));
			}

			terms_map.insert("version".to_owned(), "latest".into());

			params.insert(
				AuthType::Terms.as_str().to_owned(),
				serde_json::json!({
					"policies": terms_map,
				}),
			);

			for flow in &mut flows {
				flow.stages.insert(0, AuthType::Terms);
			}
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
