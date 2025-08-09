use std::collections::BTreeMap;

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{Err, Error, Result};
use futures::StreamExt;
use ruma::{
	OwnedRoomId,
	api::{
		client::{
			error::ErrorKind,
			membership::mutual_rooms,
			profile::{
				delete_profile_key, delete_timezone_key, get_profile_key, get_timezone_key,
				set_profile_key, set_timezone_key,
			},
		},
		federation,
	},
	presence::PresenceState,
};

use super::{update_avatar_url, update_displayname};
use crate::Ruma;

/// # `GET /_matrix/client/unstable/uk.half-shot.msc2666/user/mutual_rooms`
///
/// Gets all the rooms the sender shares with the specified user.
///
/// TODO: Implement pagination, currently this just returns everything
///
/// An implementation of [MSC2666](https://github.com/matrix-org/matrix-spec-proposals/pull/2666)
#[tracing::instrument(skip_all, fields(%client), name = "mutual_rooms")]
pub(crate) async fn get_mutual_rooms_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<mutual_rooms::unstable::Request>,
) -> Result<mutual_rooms::unstable::Response> {
	let sender_user = body.sender_user();

	if sender_user == body.user_id {
		return Err!(Request(Unknown("You cannot request rooms in common with yourself.")));
	}

	if !services.users.exists(&body.user_id).await {
		return Ok(mutual_rooms::unstable::Response { joined: vec![], next_batch_token: None });
	}

	let mutual_rooms: Vec<OwnedRoomId> = services
		.rooms
		.state_cache
		.get_shared_rooms(sender_user, &body.user_id)
		.map(ToOwned::to_owned)
		.collect()
		.await;

	Ok(mutual_rooms::unstable::Response {
		joined: mutual_rooms,
		next_batch_token: None,
	})
}

/// # `DELETE /_matrix/client/unstable/uk.tcpip.msc4133/profile/:user_id/us.cloke.msc4175.tz`
///
/// Deletes the `tz` (timezone) of a user, as per MSC4133 and MSC4175.
///
/// - Also makes sure other users receive the update using presence EDUs
pub(crate) async fn delete_timezone_key_route(
	State(services): State<crate::State>,
	body: Ruma<delete_timezone_key::unstable::Request>,
) -> Result<delete_timezone_key::unstable::Response> {
	let sender_user = body.sender_user();

	if *sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot update the profile of another user")));
	}

	services.users.set_timezone(&body.user_id, None);

	if services.config.allow_local_presence {
		// Presence update
		services
			.presence
			.ping_presence(&body.user_id, &PresenceState::Online)
			.await?;
	}

	Ok(delete_timezone_key::unstable::Response {})
}

/// # `PUT /_matrix/client/unstable/uk.tcpip.msc4133/profile/:user_id/us.cloke.msc4175.tz`
///
/// Updates the `tz` (timezone) of a user, as per MSC4133 and MSC4175.
///
/// - Also makes sure other users receive the update using presence EDUs
pub(crate) async fn set_timezone_key_route(
	State(services): State<crate::State>,
	body: Ruma<set_timezone_key::unstable::Request>,
) -> Result<set_timezone_key::unstable::Response> {
	let sender_user = body.sender_user();

	if *sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot update the profile of another user")));
	}

	services.users.set_timezone(&body.user_id, body.tz.clone());

	if services.config.allow_local_presence {
		// Presence update
		services
			.presence
			.ping_presence(&body.user_id, &PresenceState::Online)
			.await?;
	}

	Ok(set_timezone_key::unstable::Response {})
}

/// # `PUT /_matrix/client/unstable/uk.tcpip.msc4133/profile/{user_id}/{field}`
///
/// Updates the profile key-value field of a user, as per MSC4133.
///
/// This also handles the avatar_url and displayname being updated.
pub(crate) async fn set_profile_key_route(
	State(services): State<crate::State>,
	body: Ruma<set_profile_key::unstable::Request>,
) -> Result<set_profile_key::unstable::Response> {
	let sender_user = body.sender_user();

	if *sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot update the profile of another user")));
	}

	if body.kv_pair.is_empty() {
		return Err!(Request(BadJson(
			"The key-value pair JSON body is empty. Use DELETE to delete a key"
		)));
	}

	if body.kv_pair.len() > 1 {
		// TODO: support PATCH or "recursively" adding keys in some sort
		return Err!(Request(BadJson(
			"This endpoint can only take one key-value pair at a time"
		)));
	}

	let Some(profile_key_value) = body.kv_pair.get(&body.key_name) else {
		return Err!(Request(BadJson(
			"The key does not match the URL field key, or JSON body is empty (use DELETE)"
		)));
	};

	if body
		.kv_pair
		.keys()
		.any(|key| key.starts_with("u.") && !profile_key_value.is_string())
	{
		return Err!(Request(BadJson("u.* profile key fields must be strings")));
	}

	if body.kv_pair.keys().any(|key| key.len() > 128) {
		return Err!(Request(BadJson("Key names cannot be longer than 128 bytes")));
	}

	if body.key_name == "displayname" {
		let all_joined_rooms: Vec<OwnedRoomId> = services
			.rooms
			.state_cache
			.rooms_joined(&body.user_id)
			.map(Into::into)
			.collect()
			.await;

		update_displayname(
			&services,
			&body.user_id,
			Some(profile_key_value.to_string()),
			&all_joined_rooms,
		)
		.await;
	} else if body.key_name == "avatar_url" {
		let mxc = ruma::OwnedMxcUri::from(profile_key_value.to_string());

		let all_joined_rooms: Vec<OwnedRoomId> = services
			.rooms
			.state_cache
			.rooms_joined(&body.user_id)
			.map(Into::into)
			.collect()
			.await;

		update_avatar_url(&services, &body.user_id, Some(mxc), None, &all_joined_rooms).await;
	} else {
		services.users.set_profile_key(
			&body.user_id,
			&body.key_name,
			Some(profile_key_value.clone()),
		);
	}

	if services.config.allow_local_presence {
		// Presence update
		services
			.presence
			.ping_presence(&body.user_id, &PresenceState::Online)
			.await?;
	}

	Ok(set_profile_key::unstable::Response {})
}

/// # `DELETE /_matrix/client/unstable/uk.tcpip.msc4133/profile/{user_id}/{field}`
///
/// Deletes the profile key-value field of a user, as per MSC4133.
///
/// This also handles the avatar_url and displayname being updated.
pub(crate) async fn delete_profile_key_route(
	State(services): State<crate::State>,
	body: Ruma<delete_profile_key::unstable::Request>,
) -> Result<delete_profile_key::unstable::Response> {
	let sender_user = body.sender_user();

	if *sender_user != body.user_id && body.appservice_info.is_none() {
		return Err!(Request(Forbidden("You cannot update the profile of another user")));
	}

	if body.kv_pair.len() > 1 {
		// TODO: support PATCH or "recursively" adding keys in some sort
		return Err!(Request(BadJson(
			"This endpoint can only take one key-value pair at a time"
		)));
	}

	if body.key_name == "displayname" {
		let all_joined_rooms: Vec<OwnedRoomId> = services
			.rooms
			.state_cache
			.rooms_joined(&body.user_id)
			.map(Into::into)
			.collect()
			.await;

		update_displayname(&services, &body.user_id, None, &all_joined_rooms).await;
	} else if body.key_name == "avatar_url" {
		let all_joined_rooms: Vec<OwnedRoomId> = services
			.rooms
			.state_cache
			.rooms_joined(&body.user_id)
			.map(Into::into)
			.collect()
			.await;

		update_avatar_url(&services, &body.user_id, None, None, &all_joined_rooms).await;
	} else {
		services
			.users
			.set_profile_key(&body.user_id, &body.key_name, None);
	}

	if services.config.allow_local_presence {
		// Presence update
		services
			.presence
			.ping_presence(&body.user_id, &PresenceState::Online)
			.await?;
	}

	Ok(delete_profile_key::unstable::Response {})
}

/// # `GET /_matrix/client/unstable/uk.tcpip.msc4133/profile/:user_id/us.cloke.msc4175.tz`
///
/// Returns the `timezone` of the user as per MSC4133 and MSC4175.
///
/// - If user is on another server and we do not have a local copy already fetch
///   `timezone` over federation
pub(crate) async fn get_timezone_key_route(
	State(services): State<crate::State>,
	body: Ruma<get_timezone_key::unstable::Request>,
) -> Result<get_timezone_key::unstable::Response> {
	if !services.globals.user_is_local(&body.user_id) {
		// Create and update our local copy of the user
		if let Ok(response) = services
			.sending
			.send_federation_request(
				body.user_id.server_name(),
				federation::query::get_profile_information::v1::Request {
					user_id: body.user_id.clone(),
					field: None, // we want the full user's profile to update locally as well
				},
			)
			.await
		{
			if !services.users.exists(&body.user_id).await {
				services.users.create(&body.user_id, None, None).await?;
			}

			services
				.users
				.set_displayname(&body.user_id, response.displayname.clone());

			services
				.users
				.set_avatar_url(&body.user_id, response.avatar_url.clone());

			services
				.users
				.set_blurhash(&body.user_id, response.blurhash.clone());

			services
				.users
				.set_timezone(&body.user_id, response.tz.clone());

			return Ok(get_timezone_key::unstable::Response { tz: response.tz });
		}
	}

	if !services.users.exists(&body.user_id).await {
		// Return 404 if this user doesn't exist and we couldn't fetch it over
		// federation
		return Err(Error::BadRequest(ErrorKind::NotFound, "Profile was not found."));
	}

	Ok(get_timezone_key::unstable::Response {
		tz: services.users.timezone(&body.user_id).await.ok(),
	})
}

/// # `GET /_matrix/client/unstable/uk.tcpip.msc4133/profile/{userId}/{field}}`
///
/// Gets the profile key-value field of a user, as per MSC4133.
///
/// - If user is on another server and we do not have a local copy already fetch
///   `timezone` over federation
pub(crate) async fn get_profile_key_route(
	State(services): State<crate::State>,
	body: Ruma<get_profile_key::unstable::Request>,
) -> Result<get_profile_key::unstable::Response> {
	let mut profile_key_value: BTreeMap<String, serde_json::Value> = BTreeMap::new();

	if !services.globals.user_is_local(&body.user_id) {
		// Create and update our local copy of the user
		if let Ok(response) = services
			.sending
			.send_federation_request(
				body.user_id.server_name(),
				federation::query::get_profile_information::v1::Request {
					user_id: body.user_id.clone(),
					field: None, // we want the full user's profile to update locally as well
				},
			)
			.await
		{
			if !services.users.exists(&body.user_id).await {
				services.users.create(&body.user_id, None, None).await?;
			}

			services
				.users
				.set_displayname(&body.user_id, response.displayname.clone());

			services
				.users
				.set_avatar_url(&body.user_id, response.avatar_url.clone());

			services
				.users
				.set_blurhash(&body.user_id, response.blurhash.clone());

			services
				.users
				.set_timezone(&body.user_id, response.tz.clone());

			match response.custom_profile_fields.get(&body.key_name) {
				| Some(value) => {
					profile_key_value.insert(body.key_name.clone(), value.clone());
					services.users.set_profile_key(
						&body.user_id,
						&body.key_name,
						Some(value.clone()),
					);
				},
				| _ => {
					return Err!(Request(NotFound("The requested profile key does not exist.")));
				},
			}

			if profile_key_value.is_empty() {
				return Err!(Request(NotFound("The requested profile key does not exist.")));
			}

			return Ok(get_profile_key::unstable::Response { value: profile_key_value });
		}
	}

	if !services.users.exists(&body.user_id).await {
		// Return 404 if this user doesn't exist and we couldn't fetch it over
		// federation
		return Err!(Request(NotFound("Profile was not found.")));
	}

	match services
		.users
		.profile_key(&body.user_id, &body.key_name)
		.await
	{
		| Ok(value) => {
			profile_key_value.insert(body.key_name.clone(), value);
		},
		| _ => {
			return Err!(Request(NotFound("The requested profile key does not exist.")));
		},
	}

	if profile_key_value.is_empty() {
		return Err!(Request(NotFound("The requested profile key does not exist.")));
	}

	Ok(get_profile_key::unstable::Response { value: profile_key_value })
}
