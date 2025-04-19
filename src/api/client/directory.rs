use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Result, err, info,
	utils::{
		TryFutureExtExt,
		math::Expected,
		result::FlatOk,
		stream::{ReadyExt, WidebandExt},
	},
};
use conduwuit_service::Services;
use futures::{
	FutureExt, StreamExt, TryFutureExt,
	future::{join, join4, join5},
};
use ruma::{
	OwnedRoomId, RoomId, ServerName, UInt, UserId,
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
	directory::{Filter, PublicRoomJoinRule, PublicRoomsChunk, RoomNetwork, RoomTypeFilter},
	events::{
		StateEventType,
		room::{
			join_rules::{JoinRule, RoomJoinRulesEventContent},
			power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent},
		},
	},
	uint,
};

use crate::Ruma;

/// # `POST /_matrix/client/v3/publicRooms`
///
/// Lists the public rooms on this server.
///
/// - Rooms are ordered by the number of joined members
#[tracing::instrument(skip_all, fields(%client), name = "publicrooms")]
pub(crate) async fn get_public_rooms_filtered_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
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
#[tracing::instrument(skip_all, fields(%client), name = "publicrooms")]
pub(crate) async fn get_public_rooms_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
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

	Ok(get_public_rooms::v3::Response {
		chunk: response.chunk,
		prev_batch: response.prev_batch,
		next_batch: response.next_batch,
		total_room_count_estimate: response.total_room_count_estimate,
	})
}

/// # `PUT /_matrix/client/r0/directory/list/room/{roomId}`
///
/// Sets the visibility of a given room in the room directory.
#[tracing::instrument(skip_all, fields(%client), name = "room_directory")]
pub(crate) async fn set_room_visibility_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<set_room_visibility::v3::Request>,
) -> Result<set_room_visibility::v3::Response> {
	let sender_user = body.sender_user();

	if !services.rooms.metadata.exists(&body.room_id).await {
		// Return 404 if the room doesn't exist
		return Err!(Request(NotFound("Room not found")));
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

	Ok(set_room_visibility::v3::Response {})
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

	Ok(get_room_visibility::v3::Response {
		visibility: if services.rooms.directory.is_public_room(&body.room_id).await {
			room::Visibility::Public
		} else {
			room::Visibility::Private
		},
	})
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
				federation::directory::get_public_rooms_filtered::v1::Request {
					limit,
					since: since.map(ToOwned::to_owned),
					filter: Filter {
						generic_search_term: filter.generic_search_term.clone(),
						room_types: filter.room_types.clone(),
					},
					room_network: RoomNetwork::Matrix,
				},
			)
			.await?;

		return Ok(get_public_rooms_filtered::v3::Response {
			chunk: response.chunk,
			prev_batch: response.prev_batch,
			next_batch: response.next_batch,
			total_room_count_estimate: response.total_room_count_estimate,
		});
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
		.map(ToOwned::to_owned)
		.wide_then(|room_id| public_rooms_chunk(services, room_id))
		.ready_filter_map(|chunk| {
			if !filter.room_types.is_empty() && !filter.room_types.contains(&RoomTypeFilter::from(chunk.room_type.clone())) {
				return None;
			}

			if let Some(query) = filter.generic_search_term.as_ref().map(|q| q.to_lowercase()) {
				if let Some(name) = &chunk.name {
					if name.as_str().to_lowercase().contains(&query) {
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

	Ok(get_public_rooms_filtered::v3::Response {
		chunk,
		prev_batch,
		next_batch,
		total_room_count_estimate,
	})
}

/// Check whether the user can publish to the room directory via power levels of
/// room history visibility event or room creator
async fn user_can_publish_room(
	services: &Services,
	user_id: &UserId,
	room_id: &RoomId,
) -> Result<bool> {
	match services
		.rooms
		.state_accessor
		.room_state_get(room_id, &StateEventType::RoomPowerLevels, "")
		.await
	{
		| Ok(event) => serde_json::from_str(event.content.get())
			.map_err(|_| err!(Database("Invalid event content for m.room.power_levels")))
			.map(|content: RoomPowerLevelsEventContent| {
				RoomPowerLevels::from(content)
					.user_can_send_state(user_id, StateEventType::RoomHistoryVisibility)
			}),
		| _ => {
			match services
				.rooms
				.state_accessor
				.room_state_get(room_id, &StateEventType::RoomCreate, "")
				.await
			{
				| Ok(event) => Ok(event.sender == user_id),
				| _ => Err!(Request(Forbidden("User is not allowed to publish this room"))),
			}
		},
	}
}

async fn public_rooms_chunk(services: &Services, room_id: OwnedRoomId) -> PublicRoomsChunk {
	let name = services.rooms.state_accessor.get_name(&room_id).ok();

	let room_type = services.rooms.state_accessor.get_room_type(&room_id).ok();

	let canonical_alias = services
		.rooms
		.state_accessor
		.get_canonical_alias(&room_id)
		.ok();

	let avatar_url = services.rooms.state_accessor.get_avatar(&room_id);

	let topic = services.rooms.state_accessor.get_room_topic(&room_id).ok();

	let world_readable = services.rooms.state_accessor.is_world_readable(&room_id);

	let join_rule = services
		.rooms
		.state_accessor
		.room_state_get_content(&room_id, &StateEventType::RoomJoinRules, "")
		.map_ok(|c: RoomJoinRulesEventContent| match c.join_rule {
			| JoinRule::Public => PublicRoomJoinRule::Public,
			| JoinRule::Knock => "knock".into(),
			| JoinRule::KnockRestricted(_) => "knock_restricted".into(),
			| _ => "invite".into(),
		});

	let guest_can_join = services.rooms.state_accessor.guest_can_join(&room_id);

	let num_joined_members = services.rooms.state_cache.room_joined_count(&room_id);

	let (
		(avatar_url, canonical_alias, guest_can_join, join_rule, name),
		(num_joined_members, room_type, topic, world_readable),
	) = join(
		join5(avatar_url, canonical_alias, guest_can_join, join_rule, name),
		join4(num_joined_members, room_type, topic, world_readable),
	)
	.boxed()
	.await;

	PublicRoomsChunk {
		avatar_url: avatar_url.into_option().unwrap_or_default().url,
		canonical_alias,
		guest_can_join,
		join_rule: join_rule.unwrap_or_default(),
		name,
		num_joined_members: num_joined_members
			.map(TryInto::try_into)
			.map(Result::ok)
			.flat_ok()
			.unwrap_or_else(|| uint!(0)),
		room_id,
		room_type,
		topic,
		world_readable,
	}
}
