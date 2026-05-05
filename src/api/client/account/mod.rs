use axum::extract::State;
use axum_client_ip::ClientIp;
use conduwuit::{
	Err, Result, err, info,
	pdu::PartialPdu,
	utils::{ReadyExt, stream::BroadbandExt},
};
use conduwuit_service::Services;
use futures::{FutureExt, StreamExt};
use lettre::{Address, message::Mailbox};
use ruma::{
	OwnedRoomId, UserId,
	api::client::{
		account::{
			ThirdPartyIdRemovalStatus, change_password, check_registration_token_validity,
			deactivate, get_username_availability, request_password_change_token_via_email,
			whoami,
		},
		uiaa::{AuthFlow, AuthType},
	},
	assign,
	events::room::{
		member::{MembershipState, RoomMemberEventContent},
		power_levels::RoomPowerLevelsEventContent,
	},
};
use service::{mailer::messages, uiaa::Identity, users::HashedPassword};

use super::{DEVICE_ID_LENGTH, TOKEN_LENGTH, join_room_by_id_helper};
use crate::Ruma;

pub(crate) mod register;
pub(crate) mod threepid;

/// # `GET /_matrix/client/v3/register/available`
///
/// Checks if a username is valid and available on this server.
///
/// Conditions for returning true:
/// - The user id is not historical
/// - The server name of the user id matches this server
/// - No user or appservice on this server already claimed this username
///
/// Note: This will not reserve the username, so the username might become
/// invalid when trying to register
#[tracing::instrument(skip_all, fields(%client), name = "register_available", level = "info")]
pub(crate) async fn get_register_available_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<get_username_availability::v3::Request>,
) -> Result<get_username_availability::v3::Response> {
	// Validate user id
	let user_id =
		match UserId::parse_with_server_name(&body.username, services.globals.server_name()) {
			| Ok(user_id) => {
				if let Err(e) = user_id.validate_strict() {
					return Err!(Request(InvalidUsername(debug_warn!(
						"Username {} contains disallowed characters or spaces: {e}",
						body.username
					))));
				}

				user_id
			},
			| Err(e) => {
				return Err!(Request(InvalidUsername(debug_warn!(
					"Username {} is not valid: {e}",
					body.username
				))));
			},
		};

	// Check if username is creative enough
	if services.users.exists(&user_id).await {
		return Err!(Request(UserInUse("User ID is not available.")));
	}

	if let Some(ref info) = body.appservice_info {
		if !info.is_user_match(&user_id) {
			return Err!(Request(Exclusive("Username is not in an appservice namespace.")));
		}
	}

	if services.appservice.is_exclusive_user_id(&user_id).await {
		return Err!(Request(Exclusive("Username is reserved by an appservice.")));
	}

	Ok(get_username_availability::v3::Response::new(true))
}

/// # `POST /_matrix/client/r0/account/password`
///
/// Changes the password of this account.
///
/// - Requires UIAA to verify user password
/// - Changes the password of the sender user
/// - The password hash is calculated using argon2 with 32 character salt, the
///   plain password is
/// not saved
///
/// If logout_devices is true it does the following for each device except the
/// sender device:
/// - Invalidates access token
/// - Deletes device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets to-device events
/// - Triggers device list updates
#[tracing::instrument(skip_all, fields(%client), name = "change_password", level = "info")]
pub(crate) async fn change_password_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<change_password::v3::Request>,
) -> Result<change_password::v3::Response> {
	let identity = if let Some(ref user_id) = body.sender_user {
		// A signed-in user is trying to change their password, prompt them for their
		// existing one

		services
			.uiaa
			.authenticate(
				&body.auth,
				vec![AuthFlow::new(vec![AuthType::Password])],
				Box::default(),
				Some(Identity::from_user_id(user_id)),
			)
			.await?
	} else {
		// A signed-out user is trying to reset their password, prompt them for email
		// confirmation. Note that we do not _send_ an email here, their client should
		// have already hit `/account/password/requestToken` to send the email. We
		// just validate it.

		services
			.uiaa
			.authenticate(
				&body.auth,
				vec![AuthFlow::new(vec![AuthType::EmailIdentity])],
				Box::default(),
				None,
			)
			.await?
	};

	let sender_user = UserId::parse(format!(
		"@{}:{}",
		identity.localpart.expect("localpart should be known"),
		services.globals.server_name()
	))
	.expect("user ID should be valid");

	services
		.users
		.set_password(&sender_user, Some(HashedPassword::new(&body.new_password)?));

	if body.logout_devices {
		// Logout all devices except the current one
		services
			.users
			.all_device_ids(&sender_user)
			.ready_filter(|id| *id != body.sender_device())
			.for_each(async |id| services.users.remove_device(&sender_user, &id).await)
			.await;

		// Remove all pushers except the ones associated with this session
		services
			.pusher
			.get_pushkeys(&sender_user)
			.map(ToOwned::to_owned)
			.broad_filter_map(async |pushkey| {
				services
					.pusher
					.get_pusher_device(&pushkey)
					.await
					.ok()
					.as_ref()
					.is_some_and(|pusher_device| pusher_device != body.sender_device())
					.then_some(pushkey)
			})
			.for_each(async |pushkey| {
				services.pusher.delete_pusher(&sender_user, &pushkey).await;
			})
			.await;
	}

	info!("User {} changed their password.", &sender_user);

	if services.server.config.admin_room_notices {
		services
			.admin
			.notice(&format!("User {} changed their password.", &sender_user))
			.await;
	}

	Ok(change_password::v3::Response::new())
}

/// # `POST /_matrix/client/v3/account/password/email/requestToken`
///
/// Requests a validation email for the purpose of resetting a user's password.
pub(crate) async fn request_password_change_token_via_email_route(
	State(services): State<crate::State>,
	body: Ruma<request_password_change_token_via_email::v3::Request>,
) -> Result<request_password_change_token_via_email::v3::Response> {
	let Ok(email) = Address::try_from(body.email.clone()) else {
		return Err!(Request(InvalidParam("Invalid email address.")));
	};

	let Some(localpart) = services.threepid.get_localpart_for_email(&email).await else {
		return Err!(Request(ThreepidNotFound(
			"No account is associated with this email address"
		)));
	};

	let user_id =
		UserId::parse(format!("@{localpart}:{}", services.globals.server_name())).unwrap();
	let display_name = services.users.displayname(&user_id).await.ok();

	let session = services
		.threepid
		.send_validation_email(
			Mailbox::new(display_name.clone(), email),
			|verification_link| messages::PasswordReset {
				display_name: display_name.as_deref(),
				user_id: &user_id,
				verification_link,
			},
			&body.client_secret,
			body.send_attempt.try_into().unwrap(),
		)
		.await?;

	Ok(request_password_change_token_via_email::v3::Response::new(session))
}

/// # `GET /_matrix/client/v3/account/whoami`
///
/// Get `user_id` of the sender user.
///
/// Note: Also works for Application Services
pub(crate) async fn whoami_route(
	State(_): State<crate::State>,
	body: Ruma<whoami::v3::Request>,
) -> Result<whoami::v3::Response> {
	Ok(assign!(whoami::v3::Response::new(body.sender_user().to_owned(), false), {
		device_id: body.sender_device,
	}))
}

/// # `POST /_matrix/client/r0/account/deactivate`
///
/// Deactivate sender user account.
///
/// - Leaves all rooms and rejects all invitations
/// - Invalidates all access tokens
/// - Deletes all device metadata (device id, device display name, last seen ip,
///   last seen ts)
/// - Forgets all to-device events
/// - Triggers device list updates
/// - Removes ability to log in again
#[tracing::instrument(skip_all, fields(%client), name = "deactivate", level = "info")]
pub(crate) async fn deactivate_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<deactivate::v3::Request>,
) -> Result<deactivate::v3::Response> {
	// Authentication for this endpoint is technically optional,
	// but we require the user to be logged in
	let sender_user = body
		.sender_user
		.as_ref()
		.ok_or_else(|| err!(Request(MissingToken("Missing access token."))))?;

	// Prompt the user to confirm with their password using UIAA
	let _ = services
		.uiaa
		.authenticate_password(&body.auth, Some(Identity::from_user_id(sender_user)))
		.await?;

	// Remove profile pictures and display name
	let all_joined_rooms: Vec<OwnedRoomId> = services
		.rooms
		.state_cache
		.rooms_joined(sender_user)
		.map(Into::into)
		.collect()
		.await;

	full_user_deactivate(&services, sender_user, &all_joined_rooms)
		.boxed()
		.await?;

	info!("User {sender_user} deactivated their account.");

	if services.server.config.admin_room_notices {
		services
			.admin
			.notice(&format!("User {sender_user} deactivated their account."))
			.await;
	}

	Ok(deactivate::v3::Response::new(ThirdPartyIdRemovalStatus::Success))
}

/// # `GET /_matrix/client/v1/register/m.login.registration_token/validity`
///
/// Checks if the provided registration token is valid at the time of checking.
pub(crate) async fn check_registration_token_validity(
	State(services): State<crate::State>,
	body: Ruma<check_registration_token_validity::v1::Request>,
) -> Result<check_registration_token_validity::v1::Response> {
	// TODO: ratelimit this pretty heavily

	let valid = services
		.registration_tokens
		.validate_token(body.token.clone())
		.await
		.is_some();

	Ok(check_registration_token_validity::v1::Response::new(valid))
}

/// Runs through all the deactivation steps:
///
/// - Mark as deactivated
/// - Removing display name
/// - Removing avatar URL and blurhash
/// - Removing all profile data
/// - Leaving all rooms (and forgets all of them)
pub async fn full_user_deactivate(
	services: &Services,
	user_id: &UserId,
	all_joined_rooms: &[OwnedRoomId],
) -> Result<()> {
	services.users.deactivate_account(user_id).await.ok();

	if services.globals.user_is_local(user_id) {
		let _ = services
			.threepid
			.disassociate_localpart_email(user_id.localpart())
			.await;
	}

	services.users.clear_profile(user_id).await;

	services
		.pusher
		.get_pushkeys(user_id)
		.for_each(async |pushkey| {
			services.pusher.delete_pusher(user_id, pushkey).await;
		})
		.await;

	// TODO: Rescind all user invites

	let mut pdu_queue: Vec<(PartialPdu, &OwnedRoomId)> = Vec::new();

	for room_id in all_joined_rooms {
		let room_power_levels = services
			.rooms
			.state_accessor
			.get_room_power_levels(room_id)
			.await;

		let user_can_demote_self =
			room_power_levels.user_can_change_user_power_level(user_id, user_id);

		if user_can_demote_self
			&& let Ok(mut power_levels_content) =
				RoomPowerLevelsEventContent::try_from(room_power_levels)
		{
			power_levels_content.users.remove(user_id);
			let pl_evt = PartialPdu::state(String::new(), &power_levels_content);
			pdu_queue.push((pl_evt, room_id));
		}

		// Leave the room
		pdu_queue.push((
			PartialPdu::state(
				user_id.to_string(),
				&RoomMemberEventContent::new(MembershipState::Leave),
			),
			room_id,
		));

		// TODO: Redact all messages sent by the user in the room
	}

	for (pdu, room_id) in pdu_queue {
		let state_lock = services.rooms.state.mutex.lock(room_id.as_str()).await;

		let _ = services
			.rooms
			.timeline
			.build_and_append_pdu(pdu, user_id, Some(room_id.as_ref()), &state_lock)
			.await;
	}

	for room_id in all_joined_rooms {
		services.rooms.state_cache.forget(room_id, user_id);
	}

	Ok(())
}
