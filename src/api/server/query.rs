use axum::extract::State;
use conduwuit::{Err, Result, err};
use futures::StreamExt;
use rand::seq::SliceRandom;
use ruma::{
	OwnedServerName,
	api::federation::query::{get_profile_information, get_room_information},
};

use crate::{
	Ruma,
	client::{get_local_profile, get_local_profile_field},
};

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

	Ok(get_room_information::v1::Response::new(room_id, servers))
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
		return Err!(Request(Forbidden(
			"Profile lookup over federation is not allowed on this homeserver."
		)));
	}

	if !services.globals.server_is_ours(body.user_id.server_name()) {
		return Err!(Request(InvalidParam("User does not belong to this server.")));
	}

	let response = if let Some(field) = &body.field {
		let mut response = get_profile_information::v1::Response::new();

		if let Some(value) =
			get_local_profile_field(&services, &body.user_id, field.to_owned()).await
		{
			response.set(value.field_name().as_str().to_owned(), value.value().into_owned());
		}

		response
	} else {
		get_local_profile(&services, &body.user_id)
			.await
			.into_iter()
			.collect()
	};

	Ok(response)
}
