use axum::extract::State;
use conduwuit::{
	Err, EventTypeExt, Result, debug, debug_warn, err, info,
	matrix::{
		event::gen_event_id,
		pdu::{PartialPdu, PduEvent},
	},
	result::FlatOk,
	trace,
	utils::{self, shuffle, stream::IterStream, to_canonical_object},
	warn,
};
use futures::{FutureExt, StreamExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedRoomId, OwnedServerName, OwnedUserId, RoomId,
	RoomVersionId, UserId,
	api::{
		client::knock::knock_room,
		error::{ErrorKind, IncompatibleRoomVersionErrorData},
		federation,
	},
	canonical_json::to_canonical_value,
	events::{
		StateEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
};
use service::{
	Services,
	rooms::{membership::validate_remote_member_event_stub, state::RoomMutexGuard},
};

use super::banned_room_check;
use crate::{Ruma, server::validate_partial_membership_state};

/// # `POST /_matrix/client/*/knock/{roomIdOrAlias}`
///
/// Tries to knock the room to ask permission to join for the sender user.
#[tracing::instrument(skip_all, name = "knock", level = "info")]
pub(crate) async fn knock_room_route(
	State(services): State<crate::State>,
	body: Ruma<knock_room::v3::Request>,
) -> Result<knock_room::v3::Response> {
	let sender_user = body.identity.expect_sender_user()?;
	let body = &body.body;
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let (servers, room_id) = match OwnedRoomId::try_from(body.room_id_or_alias.clone()) {
		| Ok(room_id) => {
			banned_room_check(&services, sender_user, Some(&room_id), room_id.server_name())
				.await?;

			let mut servers = body.via.clone();
			servers.extend(
				services
					.rooms
					.state_cache
					.servers_invite_via(&room_id)
					.collect::<Vec<_>>()
					.await,
			);

			servers.extend(
				services
					.rooms
					.state_cache
					.invite_state(sender_user, &room_id)
					.await
					.unwrap_or_default()
					.iter()
					.filter_map(|event| event.get_field("sender").ok().flatten())
					.filter_map(|sender: &str| UserId::parse(sender).ok())
					.map(|user| user.server_name().to_owned()),
			);

			if let Some(server) = room_id.server_name() {
				servers.push(server.to_owned());
			}

			servers.sort_unstable();
			servers.dedup();
			shuffle(&mut servers);

			(servers, room_id)
		},
		| Err(room_alias) => {
			let (room_id, mut servers) = services.rooms.alias.resolve_alias(&room_alias).await?;

			banned_room_check(
				&services,
				sender_user,
				Some(&room_id),
				Some(room_alias.server_name()),
			)
			.await?;

			let addl_via_servers = services.rooms.state_cache.servers_invite_via(&room_id);

			let addl_state_servers = services
				.rooms
				.state_cache
				.invite_state(sender_user, &room_id)
				.await
				.unwrap_or_default();

			let mut addl_servers: Vec<_> = addl_state_servers
				.iter()
				.map(|event| event.get_field("sender"))
				.filter_map(FlatOk::flat_ok)
				.map(|user: OwnedUserId| user.server_name().to_owned())
				.stream()
				.chain(addl_via_servers)
				.collect()
				.await;

			addl_servers.sort_unstable();
			addl_servers.dedup();
			shuffle(&mut addl_servers);
			servers.append(&mut addl_servers);

			(servers, room_id)
		},
	};

	knock_room_by_id_helper(&services, sender_user, &room_id, body.reason.clone(), &servers)
		.boxed()
		.await
}

async fn knock_room_by_id_helper(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	servers: &[OwnedServerName],
) -> Result<knock_room::v3::Response> {
	let state_lock = services.rooms.state.mutex.lock(room_id).await;

	if services
		.rooms
		.state_cache
		.is_invited(sender_user, room_id)
		.await
	{
		debug_warn!("{sender_user} is already invited in {room_id} but attempted to knock");
		return Err!(Request(Forbidden(
			"You cannot knock on a room you are already invited/accepted to."
		)));
	}

	if services
		.rooms
		.state_cache
		.is_joined(sender_user, room_id)
		.await
	{
		debug_warn!("{sender_user} is already joined in {room_id} but attempted to knock");
		return Err!(Request(Forbidden("You cannot knock on a room you are already joined in.")));
	}

	if services
		.rooms
		.state_cache
		.is_knocked(sender_user, room_id)
		.await
	{
		debug_warn!("{sender_user} is already knocked in {room_id}");
		return Ok(knock_room::v3::Response::new(room_id.into()));
	}

	if let Ok(membership) = services
		.rooms
		.state_accessor
		.get_member(room_id, sender_user)
		.await
	{
		if membership.membership == MembershipState::Ban {
			debug_warn!("{sender_user} is banned from {room_id} but attempted to knock");
			return Err!(Request(Forbidden("You cannot knock on a room you are banned from.")));
		}
	}

	let server_in_room = services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), room_id)
		.await;

	if !server_in_room
		&& !servers
			.iter()
			.any(|sn| !services.globals.server_is_ours(sn))
	{
		return Err!(Request(NotFound(
			"Room {room_id} not found and no remote servers were provided to discover it"
		)));
	} else if server_in_room {
		// Local knocks can't fail unless the room is corrupted, so we don't
		// fall back to remote knocks like we do with remote joins.
		knock_room_helper_local(services, sender_user, room_id, reason, state_lock)
			.boxed()
			.await?;
	} else {
		knock_room_helper_remote(services, sender_user, room_id, reason, servers)
			.boxed()
			.await?;
	}

	Ok(knock_room::v3::Response::new(room_id.to_owned()))
}

#[tracing::instrument(name = "local", skip(services, reason, state_lock))]
async fn knock_room_helper_local(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	state_lock: RoomMutexGuard,
) -> Result {
	let room_version = services.rooms.state.get_room_version(room_id).await?;
	let room_version_rules = room_version
		.rules()
		.expect("room version should have defined rules");

	if !room_version_rules.authorization.knocking {
		return Err!(Request(Forbidden("This room does not support knocking.")));
	}

	let mut content = RoomMemberEventContent::new(MembershipState::Knock);
	content.displayname = services.users.displayname(sender_user).await.ok();
	content.avatar_url = services.users.avatar_url(sender_user).await.ok();
	content.reason.clone_from(&reason.clone());

	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state(sender_user.to_string(), &content),
			sender_user,
			Some(room_id),
			&state_lock,
		)
		.await
		.map(|_| ())
}

#[tracing::instrument(name = "remote", skip(services, reason, servers))]
async fn knock_room_helper_remote(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	servers: &[OwnedServerName],
) -> Result {
	info!(via=?servers, "Knocking on {room_id} over federation.");

	let (mut stub, room_version, remote_server) =
		make_knock_request(services, sender_user, room_id, servers).await?;

	info!(%remote_server, "make_knock finished");

	let room_version_rules = room_version
		.rules()
		.expect("room version should have defined rules");

	if !services.server.supported_room_version(&room_version) {
		return Err!(BadServerResponse("Remote room version {room_version} is not supported"));
	}

	stub.insert(
		"origin_server_ts".to_owned(),
		CanonicalJsonValue::Integer(
			utils::millis_since_unix_epoch()
				.try_into()
				.expect("Timestamp is valid js_int value"),
		),
	);

	let mut knock_content = RoomMemberEventContent::new(MembershipState::Knock);
	knock_content.displayname = services.users.displayname(sender_user).await.ok();
	knock_content.avatar_url = services.users.avatar_url(sender_user).await.ok();
	knock_content.reason = reason;

	stub.insert(
		"content".to_owned(),
		to_canonical_value(knock_content).expect("event is valid, we just created it"),
	);

	// In order to create a compatible ref hash (EventID) the `hashes` field
	// needs to be present
	services
		.server_keys
		.hash_and_sign_event(&mut stub, &room_version_rules)?;

	let event_id = gen_event_id(&stub, &room_version_rules)?;

	// Just verify that this event isn't hilariously malformed before we sent it
	// off.
	debug!(?stub, "Parsing knock event");
	PduEvent::from_id_val(&event_id, stub.clone())
		.map_err(|e| err!(BadServerResponse("Invalid knock event PDU: {e:?}")))?;

	info!("Asking {remote_server} for send_knock in room {room_id}");
	let request = federation::membership::create_knock_event::v1::Request::new(
		room_id.to_owned(),
		event_id.clone(),
		services
			.sending
			.convert_to_outgoing_federation_event(stub.clone())
			.await,
	);

	let send_knock_response = services
		.sending
		.send_federation_request(&remote_server, request)
		.await?;

	info!("send_knock finished");

	info!("Validating send_knock response");
	services
		.rooms
		.short
		.get_or_create_shortroomid(room_id)
		.await;
	let (create_event_id, mut state) = validate_partial_membership_state(
		services,
		&send_knock_response.knock_room_state,
		&room_version_rules,
		room_id.to_owned(),
	)
	.await?;
	state.insert(StateEventType::RoomMember.with_state_key(sender_user.as_str()), stub);
	let create_event_json = state
		.get(&StateEventType::RoomCreate.with_state_key(""))
		.expect("must have create event in knock state by this point");

	// Much like when receiving an invite over federation, if we're in this room
	// we'll receive the knock PDU over /send. Otherwise, we need to add it as
	// an outlier. Since this function isn't a fallback when local knocks fail,
	// we can do this unconditionally.
	info!("Saving knock data");
	services
		.rooms
		.outlier
		.add_pdu_outlier(&create_event_id, create_event_json);
	services.rooms.state_cache.mark_as_knocked(
		sender_user,
		room_id,
		Some(state.values().collect()),
	);
	services
		.rooms
		.state_cache
		.update_joined_count(room_id)
		.await;

	services.sync.wake(sender_user).await;

	Ok(())
}

async fn make_knock_request(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	servers: &[OwnedServerName],
) -> Result<(CanonicalJsonObject, RoomVersionId, OwnedServerName)> {
	for (idx, remote_server) in servers.iter().enumerate() {
		if services.globals.server_is_ours(remote_server) {
			continue;
		}

		info!("Asking {remote_server} for make_knock ({})", idx.saturating_add(1));

		let mut request = federation::membership::prepare_knock_event::v1::Request::new(
			room_id.to_owned(),
			sender_user.to_owned(),
		);
		request.ver = services.server.supported_room_versions().collect();

		let make_knock_response = services
			.sending
			.send_federation_request(remote_server, request)
			.await;

		trace!("make_knock response: {make_knock_response:?}");
		match make_knock_response
			.and_then(|r| {
				to_canonical_object(&r.event)
					.map(|o| (o, r.room_version))
					.map_err(Into::into)
			})
			.and_then(|(stub, room_version)| {
				validate_remote_member_event_stub(
					&MembershipState::Knock,
					sender_user,
					room_id,
					&stub,
				)
				.map(|()| (stub, room_version))
			}) {
			| Ok((evt, room_version)) =>
				return Ok((evt, room_version, remote_server.to_owned())),
			| Err(e) => match e.kind() {
				| ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData {
					room_version,
					..
				}) => {
					warn!(
						"{remote_server} reports the room we are trying to knock on is \
						 v{room_version}, which we do not support."
					);
					return Err(e);
				},
				| ErrorKind::Forbidden => {
					warn!("{remote_server} refuses to let us knock: {e}.");
					return Err(e);
				},
				| ErrorKind::NotFound => {
					info!(
						"{remote_server} does not know about {room_id}: {e}. Will continue \
						 trying."
					);
				},
				| _ => {
					info!("{remote_server} failed to make_knock: {e}. Will continue trying.");
				},
			},
		}
	}

	info!("All {} servers were unable to assist in knocking {room_id} :(", servers.len());
	Err!(BadServerResponse("No server available to assist in knocking."))
}
