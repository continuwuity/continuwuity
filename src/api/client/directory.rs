use axum::extract::State;
use axum_client_ip::ClientIp;
use conduwuit::{
	Err, Result, err, info,
	utils::{
		math::Expected,
		stream::{ReadyExt, WidebandExt},
	},
};
use conduwuit_service::Services;
use futures::StreamExt;
use ruma::{
	RoomId, ServerName, UInt, UserId,
	api::{
		client::{
			directory::{
				get_public_rooms, get_public_rooms_filtered, get_room_visibility,
				set_room_visibility,
			},
			room,
		},
		federation,
	},
	assign,
	directory::{Filter, PublicRoomsChunk, RoomNetwork, RoomTypeFilter},
	events::StateEventType,
	uint,
};
use tokio::join;

use crate::Ruma;

/// # `POST /_matrix/client/v3/publicRooms`
///
/// Lists the public rooms on this server.
///
/// - Rooms are ordered by the number of joined members
#[tracing::instrument(skip_all, fields(%client), name = "publicrooms", level = "info")]
pub(crate) async fn get_public_rooms_filtered_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<get_public_rooms_filtered::v3::Request>,
) -> Result<get_public_rooms_filtered::v3::Response> {
	if let Some(server) = &body.server {
		if services
			.moderation
			.is_remote_server_room_directory_forbidden(server)
		{
			return Err!(Request(Forbidden("Server is banned on this homeserver.")));
		}
	}

	let response = get_public_rooms_filtered_helper(
		&services,
		body.server.as_deref(),
		body.limit,
		body.since.as_deref(),
		&body.filter,
		&body.room_network,
	)
	.await
	.map_err(|e| {
		err!(Request(Unknown(warn!(?body.server, "Failed to return /publicRooms: {e}"))))
	})?;

	Ok(response)
}

/// # `GET /_matrix/client/v3/publicRooms`
///
/// Lists the public rooms on this server.
///
/// - Rooms are ordered by the number of joined members
#[tracing::instrument(skip_all, fields(%client), name = "publicrooms", level = "info")]
pub(crate) async fn get_public_rooms_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<get_public_rooms::v3::Request>,
) -> Result<get_public_rooms::v3::Response> {
	if let Some(server) = &body.server {
		if services.moderation.is_remote_server_forbidden(server) {
			return Err!(Request(Forbidden("Server is banned on this homeserver.")));
		}
	}

	let response = get_public_rooms_filtered_helper(
		&services,
		body.server.as_deref(),
		body.limit,
		body.since.as_deref(),
		&Filter::default(),
		&RoomNetwork::Matrix,
	)
	.await
	.map_err(|e| {
		err!(Request(Unknown(warn!(?body.server, "Failed to return /publicRooms: {e}"))))
	})?;

	Ok(assign!(get_public_rooms::v3::Response::new(response.chunk), {
		prev_batch: response.prev_batch,
		next_batch: response.next_batch,
		total_room_count_estimate: response.total_room_count_estimate,
	}))
}

/// # `PUT /_matrix/client/r0/directory/list/room/{roomId}`
///
/// Sets the visibility of a given room in the room directory.
#[tracing::instrument(skip_all, fields(%client), name = "room_directory", level = "info")]
pub(crate) async fn set_room_visibility_route(
	State(services): State<crate::State>,
	ClientIp(client): ClientIp,
	body: Ruma<set_room_visibility::v3::Request>,
) -> Result<set_room_visibility::v3::Response> {
	let sender_user = body.sender_user();

	if !services.rooms.metadata.exists(&body.room_id).await {
		// Return 404 if the room doesn't exist
		return Err!(Request(NotFound("Room not found")));
	}
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	if services
		.users
		.is_deactivated(sender_user)
		.await
		.unwrap_or(false)
		&& body.appservice_info.is_none()
	{
		return Err!(Request(Forbidden("Guests cannot publish to room directories")));
	}

	if !user_can_publish_room(&services, sender_user, &body.room_id).await? {
		return Err!(Request(Forbidden("User is not allowed to publish this room")));
	}

	match &body.visibility {
		| room::Visibility::Public => {
			if services.server.config.lockdown_public_room_directory
				&& !services.users.is_admin(sender_user).await
				&& body.appservice_info.is_none()
			{
				info!(
					"Non-admin user {sender_user} tried to publish {0} to the room directory \
					 while \"lockdown_public_room_directory\" is enabled",
					body.room_id
				);

				if services.server.config.admin_room_notices {
					services
						.admin
						.send_text(&format!(
							"Non-admin user {sender_user} tried to publish {0} to the room \
							 directory while \"lockdown_public_room_directory\" is enabled",
							body.room_id
						))
						.await;
				}

				return Err!(Request(Forbidden(
					"Publishing rooms to the room directory is not allowed",
				)));
			}

			services.rooms.directory.set_public(&body.room_id);

			if services.server.config.admin_room_notices {
				services
					.admin
					.send_text(&format!(
						"{sender_user} made {} public to the room directory",
						body.room_id
					))
					.await;
			}
			info!("{sender_user} made {0} public to the room directory", body.room_id);
		},
		| room::Visibility::Private => services.rooms.directory.set_not_public(&body.room_id),
		| _ => {
			return Err!(Request(InvalidParam("Room visibility type is not supported.",)));
		},
	}

	Ok(set_room_visibility::v3::Response::new())
}

/// # `GET /_matrix/client/r0/directory/list/room/{roomId}`
///
/// Gets the visibility of a given room in the room directory.
pub(crate) async fn get_room_visibility_route(
	State(services): State<crate::State>,
	body: Ruma<get_room_visibility::v3::Request>,
) -> Result<get_room_visibility::v3::Response> {
	if !services.rooms.metadata.exists(&body.room_id).await {
		// Return 404 if the room doesn't exist
		return Err!(Request(NotFound("Room not found")));
	}

	let visibility = if services.rooms.directory.is_public_room(&body.room_id).await {
		room::Visibility::Public
	} else {
		room::Visibility::Private
	};

	Ok(get_room_visibility::v3::Response::new(visibility))
}

pub(crate) async fn get_public_rooms_filtered_helper(
	services: &Services,
	server: Option<&ServerName>,
	limit: Option<UInt>,
	since: Option<&str>,
	filter: &Filter,
	_network: &RoomNetwork,
) -> Result<get_public_rooms_filtered::v3::Response> {
	if let Some(other_server) =
		server.filter(|server_name| !services.globals.server_is_ours(server_name))
	{
		let response = services
			.sending
			.send_federation_request(
				other_server,
				assign!(federation::directory::get_public_rooms_filtered::v1::Request::new(), {
					limit,
					since: since.map(ToOwned::to_owned),
					filter: assign!(Filter::new(), {
						generic_search_term: filter.generic_search_term.clone(),
						room_types: filter.room_types.clone(),
					}),
					room_network: RoomNetwork::Matrix,
				}),
			)
			.await?;

		return Ok(assign!(get_public_rooms_filtered::v3::Response::new(), {
			chunk: response.chunk,
			prev_batch: response.prev_batch,
			next_batch: response.next_batch,
			total_room_count_estimate: response.total_room_count_estimate,
		}));
	}

	// Use limit or else 10, with maximum 100
	let limit: usize = limit.map_or(10_u64, u64::from).try_into()?;
	let mut num_since: usize = 0;

	if let Some(s) = &since {
		let mut characters = s.chars();
		let backwards = match characters.next() {
			| Some('n') => false,
			| Some('p') => true,
			| _ => {
				return Err!(Request(InvalidParam("Invalid `since` token")));
			},
		};

		num_since = characters
			.collect::<String>()
			.parse()
			.map_err(|_| err!(Request(InvalidParam("Invalid `since` token."))))?;

		if backwards {
			num_since = num_since.saturating_sub(limit);
		}
	}

	let mut all_rooms: Vec<PublicRoomsChunk> = services
		.rooms
		.directory
		.public_rooms()
		.wide_then(async |room_id| {
			let summary = services
				.rooms
				.summary
				.build_local_room_summary(&room_id)
				.await
				.expect("room in public room directory should exist");

			summary.into()
		})
		.ready_filter_map(|chunk: PublicRoomsChunk| {
			if !filter.room_types.is_empty() && !filter.room_types.contains(&RoomTypeFilter::from(chunk.room_type.clone())) {
				return None;
			}

			if let Some(query) = filter.generic_search_term.as_ref().map(|q| q.to_lowercase()) {
				if let Some(name) = &chunk.name {
					if name.to_lowercase().contains(&query) {
						return Some(chunk);
					}
				}

				if let Some(topic) = &chunk.topic {
					if topic.to_lowercase().contains(&query) {
						return Some(chunk);
					}
				}

				if let Some(canonical_alias) = &chunk.canonical_alias {
					if canonical_alias.as_str().to_lowercase().contains(&query) {
						return Some(chunk);
					}
				}

				return None;
			}

			// No search term
			Some(chunk)
		})
		// We need to collect all, so we can sort by member count
		.collect()
		.await;

	all_rooms.sort_by(|l, r| r.num_joined_members.cmp(&l.num_joined_members));

	let total_room_count_estimate = UInt::try_from(all_rooms.len())
		.unwrap_or_else(|_| uint!(0))
		.into();

	let chunk: Vec<_> = all_rooms.into_iter().skip(num_since).take(limit).collect();

	let prev_batch = num_since.ne(&0).then_some(format!("p{num_since}"));

	let next_batch = chunk
		.len()
		.ge(&limit)
		.then_some(format!("n{}", num_since.expected_add(limit)));

	Ok(assign!(get_public_rooms_filtered::v3::Response::new(), {
		chunk,
		prev_batch,
		next_batch,
		total_room_count_estimate,
	}))
}

/// Checks whether the given user ID is allowed to publish the target room to
/// the server's public room directory. Users are allowed to publish rooms if
/// they are server admins, room creators (in v12), or have the power level to
/// send `m.room.canonical_alias`.
async fn user_can_publish_room(
	services: &Services,
	user_id: &UserId,
	room_id: &RoomId,
) -> Result<bool> {
	if services.users.is_admin(user_id).await {
		// Server admins can always publish to their own room directory.
		return Ok(true);
	}

	let (room_version, room_creators, power_levels) = join!(
		services.rooms.state.get_room_version(room_id),
		services.rooms.state_accessor.get_room_creators(room_id),
		services.rooms.state_accessor.get_room_power_levels(room_id),
	);

	let room_version = room_version
		.as_ref()
		.map_err(|_| err!(Request(NotFound("Unknown room"))))?;
	let room_version_rules = room_version.rules().unwrap();

	if room_version_rules
		.authorization
		.explicitly_privilege_room_creators
		&& room_creators.contains(user_id)
	{
		return Ok(true);
	}

	Ok(power_levels.user_can_send_state(user_id, StateEventType::RoomCanonicalAlias))
}
