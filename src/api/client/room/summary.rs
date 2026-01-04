use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Result, debug, debug_warn, info, trace,
	utils::{IterStream, future::TryExtExt},
};
use futures::{
	FutureExt, StreamExt, TryFutureExt,
	future::{OptionFuture, join3},
	stream::FuturesUnordered,
};
use ruma::{
	OwnedServerName, RoomId, UserId,
	api::{
		client::room::get_summary,
		federation::space::{SpaceHierarchyParentSummary, get_hierarchy},
	},
	events::room::member::MembershipState,
	space::SpaceRoomJoinRule::{self, *},
};
use service::Services;

use crate::{Ruma, RumaResponse};

/// # `GET /_matrix/client/unstable/im.nheko.summary/rooms/{roomIdOrAlias}/summary`
///
/// Returns a short description of the state of a room.
///
/// This is the "wrong" endpoint that some implementations/clients may use
/// according to the MSC. Request and response bodies are the same as
/// `get_room_summary`.
///
/// An implementation of [MSC3266](https://github.com/matrix-org/matrix-spec-proposals/pull/3266)
pub(crate) async fn get_room_summary_legacy(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_summary::msc3266::Request>,
) -> Result<RumaResponse<get_summary::msc3266::Response>> {
	get_room_summary(State(services), InsecureClientIp(client), body)
		.boxed()
		.await
		.map(RumaResponse)
}

/// # `GET /_matrix/client/unstable/im.nheko.summary/summary/{roomIdOrAlias}`
/// # `GET /_matrix/client/v1/room_summary/{roomIdOrAlias}`
///
/// Returns a short description of the state of a room.
#[tracing::instrument(skip_all, fields(%client), name = "room_summary", level = "info")]
pub(crate) async fn get_room_summary(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_summary::msc3266::Request>,
) -> Result<get_summary::msc3266::Response> {
	let (room_id, servers) = services
		.rooms
		.alias
		.resolve_with_servers(&body.room_id_or_alias, Some(body.via.clone()))
		.await?;

	if services.rooms.metadata.is_banned(&room_id).await {
		return Err!(Request(Forbidden("This room is banned on this homeserver.")));
	}

	room_summary_response(&services, &room_id, &servers, body.sender_user.as_deref())
		.boxed()
		.await
}

async fn room_summary_response(
	services: &Services,
	room_id: &RoomId,
	servers: &[OwnedServerName],
	sender_user: Option<&UserId>,
) -> Result<get_summary::msc3266::Response> {
	if services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), room_id)
		.await
	{
		match local_room_summary_response(services, room_id, sender_user)
			.boxed()
			.await
		{
			| Ok(response) => return Ok(response),
			| Err(e) => {
				debug_warn!("Failed to get local room summary: {e:?}, falling back to remote");
			},
		}
	}

	let room =
		remote_room_summary_hierarchy_response(services, room_id, servers, sender_user).await?;

	Ok(get_summary::msc3266::Response {
		room_id: room_id.to_owned(),
		canonical_alias: room.canonical_alias,
		avatar_url: room.avatar_url,
		guest_can_join: room.guest_can_join,
		name: room.name,
		num_joined_members: room.num_joined_members,
		topic: room.topic,
		world_readable: room.world_readable,
		join_rule: room.join_rule,
		room_type: room.room_type,
		room_version: room.room_version,
		encryption: room.encryption,
		allowed_room_ids: room.allowed_room_ids,
		membership: sender_user.is_some().then_some(MembershipState::Leave),
	})
}

async fn local_room_summary_response(
	services: &Services,
	room_id: &RoomId,
	sender_user: Option<&UserId>,
) -> Result<get_summary::msc3266::Response> {
	trace!(
		sender_user = sender_user.map(tracing::field::display),
		"Sending local room summary response for {room_id:?}"
	);
	let (join_rule, world_readable, guest_can_join) = join3(
		services.rooms.state_accessor.get_join_rules(room_id),
		services.rooms.state_accessor.is_world_readable(room_id),
		services.rooms.state_accessor.guest_can_join(room_id),
	)
	.await;

	// Synapse allows server admins to bypass visibility checks.
	// That seems neat so we'll copy that behaviour.
	if sender_user.is_none() || !services.users.is_admin(sender_user.unwrap()).await {
		user_can_see_summary(
			services,
			room_id,
			&join_rule.clone().into(),
			guest_can_join,
			world_readable,
			join_rule.allowed_rooms(),
			sender_user,
		)
		.await?;
	}

	let canonical_alias = services
		.rooms
		.state_accessor
		.get_canonical_alias(room_id)
		.ok();

	let name = services.rooms.state_accessor.get_name(room_id).ok();

	let topic = services.rooms.state_accessor.get_room_topic(room_id).ok();

	let room_type = services.rooms.state_accessor.get_room_type(room_id).ok();

	let avatar_url = services
		.rooms
		.state_accessor
		.get_avatar(room_id)
		.map(|res| res.into_option().unwrap_or_default().url);

	let room_version = services.rooms.state.get_room_version(room_id).ok();

	let encryption = services
		.rooms
		.state_accessor
		.get_room_encryption(room_id)
		.ok();

	let num_joined_members = services
		.rooms
		.state_cache
		.room_joined_count(room_id)
		.unwrap_or(0);

	let membership: OptionFuture<_> = sender_user
		.map(|sender_user| {
			services
				.rooms
				.state_accessor
				.get_member(room_id, sender_user)
				.map_ok_or(MembershipState::Leave, |content| content.membership)
		})
		.into();

	let (
		canonical_alias,
		name,
		num_joined_members,
		topic,
		avatar_url,
		room_type,
		room_version,
		encryption,
		membership,
	) = futures::join!(
		canonical_alias,
		name,
		num_joined_members,
		topic,
		avatar_url,
		room_type,
		room_version,
		encryption,
		membership,
	);

	Ok(get_summary::msc3266::Response {
		room_id: room_id.to_owned(),
		canonical_alias,
		avatar_url,
		guest_can_join,
		name,
		num_joined_members: num_joined_members.try_into().unwrap_or_default(),
		topic,
		world_readable,
		room_type,
		room_version,
		encryption,
		membership,
		allowed_room_ids: join_rule.allowed_rooms().map(Into::into).collect(),
		join_rule: join_rule.into(),
	})
}

/// used by MSC3266 to fetch a room's info if we do not know about it
async fn remote_room_summary_hierarchy_response(
	services: &Services,
	room_id: &RoomId,
	servers: &[OwnedServerName],
	sender_user: Option<&UserId>,
) -> Result<SpaceHierarchyParentSummary> {
	trace!(sender_user = ?sender_user.map(tracing::field::display), ?servers, "Sending remote room summary response for {room_id:?}");
	if !services.config.allow_federation {
		return Err!(Request(Forbidden("Federation is disabled.")));
	}

	if services.rooms.metadata.is_disabled(room_id).await {
		return Err!(Request(Forbidden(
			"Federaton of room {room_id} is currently disabled on this server."
		)));
	}
	if servers.is_empty() {
		return Err!(Request(MissingParam(
			"No servers were provided to fetch the room over federation"
		)));
	}

	let request = get_hierarchy::v1::Request::new(room_id.to_owned());

	let mut requests: FuturesUnordered<_> = servers
		.iter()
		.map(|server| {
			info!("Fetching room summary for {room_id} from server {server}");
			services
				.sending
				.send_federation_request(server, request.clone())
				.inspect_ok(move |v| {
					debug!("Fetched room summary for {room_id} from server {server}: {v:?}");
				})
				.inspect_err(move |e| {
					info!("Failed to fetch room summary for {room_id} from server {server}: {e}");
				})
		})
		.collect();

	while let Some(Ok(response)) = requests.next().await {
		trace!("{response:?}");
		let room = response.room.clone();
		if room.room_id != room_id {
			debug_warn!(
				"Room ID {} returned does not belong to the requested room ID {}",
				room.room_id,
				room_id
			);
			continue;
		}

		if sender_user.is_none() || !services.users.is_admin(sender_user.unwrap()).await {
			return user_can_see_summary(
				services,
				room_id,
				&room.join_rule,
				room.guest_can_join,
				room.world_readable,
				room.allowed_room_ids.iter().map(AsRef::as_ref),
				sender_user,
			)
			.await
			.map(|()| room);
		}
		return Ok(room);
	}

	Err!(Request(NotFound("Room not found or is not accessible")))
}

async fn user_can_see_summary<'a, I>(
	services: &Services,
	room_id: &RoomId,
	join_rule: &SpaceRoomJoinRule,
	guest_can_join: bool,
	world_readable: bool,
	allowed_room_ids: I,
	sender_user: Option<&UserId>,
) -> Result
where
	I: Iterator<Item = &'a RoomId> + Send,
{
	let is_public_room = matches!(join_rule, Public | Knock | KnockRestricted);
	match sender_user {
		| Some(sender_user) => {
			let user_can_see_state_events = services
				.rooms
				.state_accessor
				.user_can_see_state_events(sender_user, room_id);
			let is_guest = services.users.is_deactivated(sender_user).unwrap_or(false);
			let user_in_allowed_restricted_room = allowed_room_ids
				.stream()
				.any(|room| services.rooms.state_cache.is_joined(sender_user, room));

			let (user_can_see_state_events, is_guest, user_in_allowed_restricted_room) =
				join3(user_can_see_state_events, is_guest, user_in_allowed_restricted_room)
					.boxed()
					.await;

			if user_can_see_state_events
				|| (is_guest && guest_can_join)
				|| is_public_room
				|| user_in_allowed_restricted_room
			{
				return Ok(());
			}

			Err!(Request(Forbidden("Room is not accessible")))
		},
		| None => {
			if is_public_room || world_readable {
				return Ok(());
			}

			Err!(Request(Forbidden("Room is not accessible")))
		},
	}
}
