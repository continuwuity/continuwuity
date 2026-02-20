use std::collections::BTreeMap;

use axum::extract::State;
use conduwuit::{Error, Result, err};
use futures::StreamExt;
use get_profile_information::v1::ProfileField;
use rand::seq::SliceRandom;
use ruma::{
	OwnedServerName,
	api::{
		client::error::ErrorKind,
		federation::query::{get_profile_information, get_room_information},
	},
};

use crate::Ruma;

/// # `GET /_matrix/federation/v1/query/directory`
///
/// Resolve a room alias to a room id.
pub(crate) async fn get_room_information_route(
	State(services): State<crate::State>,
	body: Ruma<get_room_information::v1::Request>,
) -> Result<get_room_information::v1::Response> {
	let room_id = services
		.rooms
		.alias
		.resolve_local_alias(&body.room_alias)
		.await
		.map_err(|_| err!(Request(NotFound("Room alias not found."))))?;

	let mut servers: Vec<OwnedServerName> = services
		.rooms
		.state_cache
		.room_servers(&room_id)
		.map(ToOwned::to_owned)
		.collect()
		.await;

	servers.sort_unstable();
	servers.dedup();

	servers.shuffle(&mut rand::rng());

	// insert our server as the very first choice if in list
	if let Some(server_index) = servers
		.iter()
		.position(|server| server == services.globals.server_name())
	{
		servers.swap_remove(server_index);
		servers.insert(0, services.globals.server_name().to_owned());
	}

	Ok(get_room_information::v1::Response { room_id, servers })
}

/// # `GET /_matrix/federation/v1/query/profile`
///
///
/// Gets information on a profile.
pub(crate) async fn get_profile_information_route(
	State(services): State<crate::State>,
	body: Ruma<get_profile_information::v1::Request>,
) -> Result<get_profile_information::v1::Response> {
	if !services
		.server
		.config
		.allow_inbound_profile_lookup_federation_requests
	{
		return Err(Error::BadRequest(
			ErrorKind::forbidden(),
			"Profile lookup over federation is not allowed on this homeserver.",
		));
	}

	if !services.globals.server_is_ours(body.user_id.server_name()) {
		return Err(Error::BadRequest(
			ErrorKind::InvalidParam,
			"User does not belong to this server.",
		));
	}

	let mut displayname = None;
	let mut avatar_url = None;
	let mut blurhash = None;
	let mut custom_profile_fields = BTreeMap::new();

	match &body.field {
		| Some(ProfileField::DisplayName) => {
			displayname = services.users.displayname(&body.user_id).await.ok();
		},
		| Some(ProfileField::AvatarUrl) => {
			avatar_url = services.users.avatar_url(&body.user_id).await.ok();
			blurhash = services.users.blurhash(&body.user_id).await.ok();
		},
		| Some(custom_field) => {
			if let Ok(value) = services
				.users
				.profile_key(&body.user_id, custom_field.as_str())
				.await
			{
				custom_profile_fields.insert(custom_field.to_string(), value);
			}
		},
		| None => {
			displayname = services.users.displayname(&body.user_id).await.ok();
			avatar_url = services.users.avatar_url(&body.user_id).await.ok();
			blurhash = services.users.blurhash(&body.user_id).await.ok();
			custom_profile_fields = services
				.users
				.all_profile_keys(&body.user_id)
				.collect()
				.await;
		},
	}

	Ok(get_profile_information::v1::Response {
		displayname,
		avatar_url,
		blurhash,
		custom_profile_fields,
	})
}
