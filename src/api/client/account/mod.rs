use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Event, Result, err, info,
	pdu::PduBuilder,
	utils::{ReadyExt, stream::BroadbandExt},
};
use conduwuit_service::Services;
use futures::{FutureExt, StreamExt};
use ruma::{
	OwnedRoomId, OwnedUserId, UserId,
	api::client::{
		account::{
			ThirdPartyIdRemovalStatus, change_password, check_registration_token_validity,
			deactivate, get_3pids, get_username_availability,
			request_3pid_management_token_via_email, request_3pid_management_token_via_msisdn,
			whoami,
		},
		uiaa::{AuthFlow, AuthType},
	},
	events::{
		StateEventType,
		room::{
			member::{MembershipState, RoomMemberEventContent},
			power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent},
		},
	},
};
use service::uiaa::Identity;

use super::{DEVICE_ID_LENGTH, TOKEN_LENGTH, join_room_by_id_helper};
use crate::Ruma;

pub(crate) mod register;

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
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_username_availability::v3::Request>,
) -> Result<get_username_availability::v3::Response> {
	// workaround for https://github.com/matrix-org/matrix-appservice-irc/issues/1780 due to inactivity of fixing the issue
	let is_matrix_appservice_irc = body.appservice_info.as_ref().is_some_and(|appservice| {
		appservice.registration.id == "irc"
			|| appservice.registration.id.contains("matrix-appservice-irc")
			|| appservice.registration.id.contains("matrix_appservice_irc")
	});

	if services
		.globals
		.forbidden_usernames()
		.is_match(&body.username)
	{
		return Err!(Request(Forbidden("Username is forbidden")));
	}

	// don't force the username lowercase if it's from matrix-appservice-irc
	let body_username = if is_matrix_appservice_irc {
		body.username.clone()
	} else {
		body.username.to_lowercase()
	};

	// Validate user id
	let user_id =
		match UserId::parse_with_server_name(&body_username, services.globals.server_name()) {
			| Ok(user_id) => {
				if let Err(e) = user_id.validate_strict() {
					// unless the username is from the broken matrix appservice IRC bridge, we
					// should follow synapse's behaviour on not allowing things like spaces
					// and UTF-8 characters in usernames
					if !is_matrix_appservice_irc {
						return Err!(Request(InvalidUsername(debug_warn!(
							"Username {body_username} contains disallowed characters or spaces: \
							 {e}"
						))));
					}
				}

				user_id
			},
			| Err(e) => {
				return Err!(Request(InvalidUsername(debug_warn!(
					"Username {body_username} is not valid: {e}"
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

	Ok(get_username_availability::v3::Response { available: true })
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
	InsecureClientIp(client): InsecureClientIp,
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
		// confirmation Note that we do not _send_ an email here, their client should
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

	let sender_user = OwnedUserId::parse(format!(
		"@{}:{}",
		identity.localpart.expect("localpart should be known"),
		services.globals.server_name()
	))
	.expect("user ID should be valid");

	services
		.users
		.set_password(&sender_user, Some(&body.new_password))
		.await?;

	if body.logout_devices {
		// Logout all devices except the current one
		services
			.users
			.all_device_ids(&sender_user)
			.ready_filter(|id| *id != body.sender_device())
			.for_each(|id| services.users.remove_device(&sender_user, id))
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
					.filter(|pusher_device| pusher_device != body.sender_device())
					.is_some()
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

	Ok(change_password::v3::Response {})
}

/// # `GET /_matrix/client/v3/account/whoami`
///
/// Get `user_id` of the sender user.
///
/// Note: Also works for Application Services
pub(crate) async fn whoami_route(
	State(services): State<crate::State>,
	body: Ruma<whoami::v3::Request>,
) -> Result<whoami::v3::Response> {
	let is_guest = services
		.users
		.is_deactivated(body.sender_user())
		.await
		.map_err(|_| {
			err!(Request(Forbidden("Application service has not registered this user.")))
		})? && body.appservice_info.is_none();
	Ok(whoami::v3::Response {
		user_id: body.sender_user().to_owned(),
		device_id: body.sender_device.clone(),
		is_guest,
	})
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
	InsecureClientIp(client): InsecureClientIp,
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

	Ok(deactivate::v3::Response {
		id_server_unbind_result: ThirdPartyIdRemovalStatus::NoSupport,
	})
}

/// # `GET _matrix/client/v3/account/3pid`
///
/// Get a list of third party identifiers associated with this account.
///
/// - Currently always returns empty list
pub(crate) async fn third_party_route(
	body: Ruma<get_3pids::v3::Request>,
) -> Result<get_3pids::v3::Response> {
	let _sender_user = body.sender_user.as_ref().expect("user is authenticated");

	Ok(get_3pids::v3::Response::new(Vec::new()))
}

/// # `POST /_matrix/client/v3/account/3pid/email/requestToken`
///
/// "This API should be used to request validation tokens when adding an email
/// address to an account"
///
/// - 403 signals that The homeserver does not allow the third party identifier
///   as a contact option.
pub(crate) async fn request_3pid_management_token_via_email_route(
	_body: Ruma<request_3pid_management_token_via_email::v3::Request>,
) -> Result<request_3pid_management_token_via_email::v3::Response> {
	Err!(Request(ThreepidDenied("Third party identifiers are not implemented")))
}

/// # `POST /_matrix/client/v3/account/3pid/msisdn/requestToken`
///
/// "This API should be used to request validation tokens when adding an phone
/// number to an account"
///
/// - 403 signals that The homeserver does not allow the third party identifier
///   as a contact option.
pub(crate) async fn request_3pid_management_token_via_msisdn_route(
	_body: Ruma<request_3pid_management_token_via_msisdn::v3::Request>,
) -> Result<request_3pid_management_token_via_msisdn::v3::Response> {
	Err!(Request(ThreepidDenied("Third party identifiers are not implemented")))
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

	Ok(check_registration_token_validity::v1::Response { valid })
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

	services
		.users
		.all_profile_keys(user_id)
		.ready_for_each(|(profile_key, _)| {
			services.users.set_profile_key(user_id, &profile_key, None);
		})
		.await;

	// TODO: Rescind all user invites

	let mut pdu_queue: Vec<(PduBuilder, &OwnedRoomId)> = Vec::new();

	for room_id in all_joined_rooms {
		let room_power_levels = services
			.rooms
			.state_accessor
			.room_state_get_content::<RoomPowerLevelsEventContent>(
				room_id,
				&StateEventType::RoomPowerLevels,
				"",
			)
			.await
			.ok();

		let user_can_demote_self =
			room_power_levels
				.as_ref()
				.is_some_and(|power_levels_content| {
					RoomPowerLevels::from(power_levels_content.clone())
						.user_can_change_user_power_level(user_id, user_id)
				}) || services
				.rooms
				.state_accessor
				.room_state_get(room_id, &StateEventType::RoomCreate, "")
				.await
				.is_ok_and(|event| event.sender() == user_id);

		if user_can_demote_self {
			let mut power_levels_content = room_power_levels.unwrap_or_default();
			power_levels_content.users.remove(user_id);
			let pl_evt = PduBuilder::state(String::new(), &power_levels_content);
			pdu_queue.push((pl_evt, room_id));
		}

		// Leave the room
		pdu_queue.push((
			PduBuilder::state(user_id.to_string(), &RoomMemberEventContent {
				avatar_url: None,
				blurhash: None,
				membership: MembershipState::Leave,
				displayname: None,
				join_authorized_via_users_server: None,
				reason: None,
				is_direct: None,
				third_party_invite: None,
				redact_events: None,
			}),
			room_id,
		));

		// TODO: Redact all messages sent by the user in the room
	}

	super::update_all_rooms(services, pdu_queue, user_id).await;
	for room_id in all_joined_rooms {
		services.rooms.state_cache.forget(room_id, user_id);
	}

	Ok(())
}
