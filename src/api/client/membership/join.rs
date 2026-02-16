use std::{borrow::Borrow, collections::HashMap, iter::once, sync::Arc};

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Result, debug, debug_info, debug_warn, err, error, info, is_true,
	matrix::{
		StateKey,
		event::{gen_event_id, gen_event_id_canonical_json},
		pdu::{PduBuilder, PduEvent},
		state_res,
	},
	result::FlatOk,
	trace,
	utils::{
		self, shuffle,
		stream::{IterStream, ReadyExt},
		to_canonical_object,
	},
	warn,
};
use futures::{FutureExt, StreamExt, TryFutureExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedRoomId, OwnedServerName, OwnedUserId, RoomId,
	RoomVersionId, UserId,
	api::{
		client::{
			error::ErrorKind,
			membership::{join_room_by_id, join_room_by_id_or_alias},
		},
		federation::{self},
	},
	canonical_json::to_canonical_value,
	events::{
		StateEventType,
		room::{
			join_rules::JoinRule,
			member::{MembershipState, RoomMemberEventContent},
		},
	},
};
use service::{
	Services,
	appservice::RegistrationInfo,
	rooms::{
		state::RoomMutexGuard,
		state_compressor::{CompressedState, HashSetCompressStateEvent},
		timeline::pdu_fits,
	},
};
use tokio::join;

use super::{banned_room_check, validate_remote_member_event_stub};
use crate::{
	Ruma,
	server::{select_authorising_user, user_can_perform_restricted_join},
};

/// # `POST /_matrix/client/r0/rooms/{roomId}/join`
///
/// Tries to join the sender user into a room.
///
/// - If the server knowns about this room: creates the join event and does auth
///   rules locally
/// - If the server does not know about the room: asks other servers over
///   federation
#[tracing::instrument(skip_all, fields(%client), name = "join", level = "info")]
pub(crate) async fn join_room_by_id_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<join_room_by_id::v3::Request>,
) -> Result<join_room_by_id::v3::Response> {
	let sender_user = body.sender_user();
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	banned_room_check(
		&services,
		sender_user,
		Some(&body.room_id),
		body.room_id.server_name(),
		client,
	)
	.await?;

	// There is no body.server_name for /roomId/join
	let mut servers: Vec<_> = services
		.rooms
		.state_cache
		.servers_invite_via(&body.room_id)
		.map(ToOwned::to_owned)
		.collect()
		.await;

	servers.extend(
		services
			.rooms
			.state_cache
			.invite_state(sender_user, &body.room_id)
			.await
			.unwrap_or_default()
			.iter()
			.filter_map(|event| event.get_field("sender").ok().flatten())
			.filter_map(|sender: &str| UserId::parse(sender).ok())
			.map(|user| user.server_name().to_owned()),
	);

	if let Some(server) = body.room_id.server_name() {
		servers.push(server.into());
	}

	servers.sort_unstable();
	servers.dedup();
	shuffle(&mut servers);

	join_room_by_id_helper(
		&services,
		sender_user,
		&body.room_id,
		body.reason.clone(),
		&servers,
		&body.appservice_info,
	)
	.boxed()
	.await
}

/// # `POST /_matrix/client/r0/join/{roomIdOrAlias}`
///
/// Tries to join the sender user into a room.
///
/// - If the server knowns about this room: creates the join event and does auth
///   rules locally
/// - If the server does not know about the room: use the server name query
///   param if specified. if not specified, asks other servers over federation
///   via room alias server name and room ID server name
#[tracing::instrument(skip_all, fields(%client), name = "join", level = "info")]
pub(crate) async fn join_room_by_id_or_alias_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<join_room_by_id_or_alias::v3::Request>,
) -> Result<join_room_by_id_or_alias::v3::Response> {
	let sender_user = body.sender_user();
	let appservice_info = &body.appservice_info;
	let body = &body.body;
	if services.users.is_suspended(sender_user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let (servers, room_id) = match OwnedRoomId::try_from(body.room_id_or_alias.clone()) {
		| Ok(room_id) => {
			banned_room_check(
				&services,
				sender_user,
				Some(&room_id),
				room_id.server_name(),
				client,
			)
			.boxed()
			.await?;

			let mut servers = body.via.clone();
			if servers.is_empty() {
				debug!("No via servers provided for join, injecting some.");
				servers.extend(
					services
						.rooms
						.state_cache
						.servers_invite_via(&room_id)
						.map(ToOwned::to_owned)
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
				client,
			)
			.await?;

			let addl_via_servers = services
				.rooms
				.state_cache
				.servers_invite_via(&room_id)
				.map(ToOwned::to_owned);

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
				.map(|user: &UserId| user.server_name().to_owned())
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

	let join_room_response = join_room_by_id_helper(
		&services,
		sender_user,
		&room_id,
		body.reason.clone(),
		&servers,
		appservice_info,
	)
	.boxed()
	.await?;

	Ok(join_room_by_id_or_alias::v3::Response { room_id: join_room_response.room_id })
}

pub async fn join_room_by_id_helper(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	servers: &[OwnedServerName],
	appservice_info: &Option<RegistrationInfo>,
) -> Result<join_room_by_id::v3::Response> {
	let state_lock = services.rooms.state.mutex.lock(room_id).await;

	let user_is_guest = services
		.users
		.is_deactivated(sender_user)
		.await
		.unwrap_or(false)
		&& appservice_info.is_none();

	if user_is_guest && !services.rooms.state_accessor.guest_can_join(room_id).await {
		return Err!(Request(Forbidden("Guests are not allowed to join this room")));
	}

	if services
		.rooms
		.state_cache
		.is_joined(sender_user, room_id)
		.await
	{
		debug_warn!("{sender_user} is already joined in {room_id}");
		return Ok(join_room_by_id::v3::Response { room_id: room_id.into() });
	}

	if let Err(e) = services
		.antispam
		.user_may_join_room(
			sender_user.to_owned(),
			room_id.to_owned(),
			services
				.rooms
				.state_cache
				.is_invited(sender_user, room_id)
				.await,
		)
		.await
	{
		warn!("Antispam prevented user {} from joining room {}: {}", sender_user, room_id, e);
		return Err!(Request(Forbidden("You are not allowed to join this room.")));
	}

	let server_in_room = services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), room_id)
		.await;

	// Only check our known membership if we're already in the room.
	// See: https://forgejo.ellis.link/continuwuation/continuwuity/issues/855
	let membership = if server_in_room {
		services
			.rooms
			.state_accessor
			.get_member(room_id, sender_user)
			.await
	} else {
		debug!("Ignoring local state for join {room_id}, we aren't in the room yet.");
		Ok(RoomMemberEventContent::new(MembershipState::Leave))
	};
	if let Ok(m) = membership {
		if m.membership == MembershipState::Ban {
			debug_warn!("{sender_user} is banned from {room_id} but attempted to join");
			// TODO: return reason
			return Err!(Request(Forbidden("You are banned from the room.")));
		}
	}

	if !server_in_room && servers.is_empty() {
		return Err!(Request(NotFound(
			"No servers were provided to assist in joining the room remotely, and we are not \
			 already participating in the room."
		)));
	}

	if services.antispam.check_all_joins() {
		if let Err(e) = services
			.antispam
			.meowlnir_accept_make_join(room_id.to_owned(), sender_user.to_owned())
			.await
		{
			warn!("Antispam prevented user {} from joining room {}: {}", sender_user, room_id, e);
			return Err!(Request(Forbidden("Antispam rejected join request.")));
		}
	}

	if server_in_room {
		join_room_by_id_helper_local(services, sender_user, room_id, reason, servers, state_lock)
			.boxed()
			.await?;
	} else {
		// Ask a remote server if we are not participating in this room
		join_room_by_id_helper_remote(
			services,
			sender_user,
			room_id,
			reason,
			servers,
			state_lock,
		)
		.boxed()
		.await?;
	}
	Ok(join_room_by_id::v3::Response::new(room_id.to_owned()))
}

#[tracing::instrument(skip_all, fields(%sender_user, %room_id), name = "join_remote", level = "info")]
async fn join_room_by_id_helper_remote(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	servers: &[OwnedServerName],
	state_lock: RoomMutexGuard,
) -> Result {
	info!("Joining {room_id} over federation.");

	let (make_join_response, remote_server) =
		make_join_request(services, sender_user, room_id, servers).await?;

	info!("make_join finished");

	let room_version_id = make_join_response.room_version.unwrap_or(RoomVersionId::V1);

	if !services.server.supported_room_version(&room_version_id) {
		// How did we get here?
		return Err!(BadServerResponse(
			"Remote room version {room_version_id} is not supported by conduwuit"
		));
	}

	let mut join_event_stub: CanonicalJsonObject =
		serde_json::from_str(make_join_response.event.get()).map_err(|e| {
			err!(BadServerResponse(warn!(
				"Invalid make_join event json received from server: {e:?}"
			)))
		})?;

	let join_authorized_via_users_server = {
		use RoomVersionId::*;
		if !matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6 | V7) {
			join_event_stub
				.get("content")
				.map(|s| {
					s.as_object()?
						.get("join_authorised_via_users_server")?
						.as_str()
				})
				.and_then(|s| OwnedUserId::try_from(s.unwrap_or_default()).ok())
		} else {
			None
		}
	};

	join_event_stub.insert(
		"origin_server_ts".to_owned(),
		CanonicalJsonValue::Integer(
			utils::millis_since_unix_epoch()
				.try_into()
				.expect("Timestamp is valid js_int value"),
		),
	);
	join_event_stub.insert(
		"content".to_owned(),
		to_canonical_value(RoomMemberEventContent {
			displayname: services.users.displayname(sender_user).await.ok(),
			avatar_url: services.users.avatar_url(sender_user).await.ok(),
			blurhash: services.users.blurhash(sender_user).await.ok(),
			reason,
			join_authorized_via_users_server: join_authorized_via_users_server.clone(),
			..RoomMemberEventContent::new(MembershipState::Join)
		})
		.expect("event is valid, we just created it"),
	);

	// We keep the "event_id" in the pdu only in v1 or
	// v2 rooms
	match room_version_id {
		| RoomVersionId::V1 | RoomVersionId::V2 => {},
		| _ => {
			join_event_stub.remove("event_id");
		},
	}

	// In order to create a compatible ref hash (EventID) the `hashes` field needs
	// to be present
	services
		.server_keys
		.hash_and_sign_event(&mut join_event_stub, &room_version_id)?;

	// Generate event id
	let event_id = gen_event_id(&join_event_stub, &room_version_id)?;

	// Add event_id back
	join_event_stub
		.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.clone().into()));

	// It has enough fields to be called a proper event now
	let mut join_event = join_event_stub;

	info!("Asking {remote_server} for send_join in room {room_id}");
	let send_join_request = federation::membership::create_join_event::v2::Request {
		room_id: room_id.to_owned(),
		event_id: event_id.clone(),
		omit_members: false,
		pdu: services
			.sending
			.convert_to_outgoing_federation_event(join_event.clone())
			.await,
	};

	let send_join_response = match services
		.sending
		.send_synapse_request(&remote_server, send_join_request)
		.await
	{
		| Ok(response) => response,
		| Err(e) => {
			error!("send_join failed: {e}");
			return Err(e);
		},
	};

	info!("send_join finished");

	if join_authorized_via_users_server.is_some() {
		if let Some(signed_raw) = &send_join_response.room_state.event {
			debug_info!(
				"There is a signed event with join_authorized_via_users_server. This room is \
				 probably using restricted joins. Adding signature to our event"
			);

			let (signed_event_id, signed_value) =
				gen_event_id_canonical_json(signed_raw, &room_version_id).map_err(|e| {
					err!(Request(BadJson(warn!(
						"Could not convert event to canonical JSON: {e}"
					))))
				})?;

			if signed_event_id != event_id {
				return Err!(Request(BadJson(warn!(
					%signed_event_id, %event_id,
					"Server {remote_server} sent event with wrong event ID"
				))));
			}

			match signed_value["signatures"]
				.as_object()
				.ok_or_else(|| {
					err!(BadServerResponse(warn!(
						"Server {remote_server} sent invalid signatures type"
					)))
				})
				.and_then(|e| {
					e.get(remote_server.as_str()).ok_or_else(|| {
						err!(BadServerResponse(warn!(
							"Server {remote_server} did not send its signature for a restricted \
							 room"
						)))
					})
				}) {
				| Ok(signature) => {
					join_event
						.get_mut("signatures")
						.expect("we created a valid pdu")
						.as_object_mut()
						.expect("we created a valid pdu")
						.insert(remote_server.to_string(), signature.clone());
				},
				| Err(e) => {
					warn!(
						"Server {remote_server} sent invalid signature in send_join signatures \
						 for event {signed_value:?}: {e:?}",
					);
				},
			}
		}
	}

	services
		.rooms
		.short
		.get_or_create_shortroomid(room_id)
		.await;

	info!("Parsing join event");
	let parsed_join_pdu = PduEvent::from_id_val(&event_id, join_event.clone())
		.map_err(|e| err!(BadServerResponse("Invalid join event PDU: {e:?}")))?;

	info!("Acquiring server signing keys for response events");
	let resp_events = &send_join_response.room_state;
	let resp_state = &resp_events.state;
	let resp_auth = &resp_events.auth_chain;
	services
		.server_keys
		.acquire_events_pubkeys(resp_auth.iter().chain(resp_state.iter()))
		.await;

	info!("Going through send_join response room_state");
	let cork = services.db.cork_and_flush();
	let state = send_join_response
		.room_state
		.state
		.iter()
		.stream()
		.then(|pdu| {
			services
				.server_keys
				.validate_and_add_event_id_no_fetch(pdu, &room_version_id)
				.inspect_err(|e| {
					debug_warn!("Could not validate send_join response room_state event: {e:?}");
				})
				.inspect(|_| debug!("Completed validating send_join response room_state event"))
		})
		.ready_filter_map(Result::ok)
		.fold(HashMap::new(), |mut state, (event_id, value)| async move {
			let pdu = match PduEvent::from_id_val(&event_id, value.clone()) {
				| Ok(pdu) => pdu,
				| Err(e) => {
					debug_warn!("Invalid PDU in send_join response: {e:?}: {value:#?}");
					return state;
				},
			};
			if !pdu_fits(&mut value.clone()) {
				warn!(
					"dropping incoming PDU {event_id} in room {room_id} from room join because \
					 it exceeds 65535 bytes or is otherwise too large."
				);
				return state;
			}
			services.rooms.outlier.add_pdu_outlier(&event_id, &value);
			if let Some(state_key) = &pdu.state_key {
				let shortstatekey = services
					.rooms
					.short
					.get_or_create_shortstatekey(&pdu.kind.to_string().into(), state_key)
					.await;

				state.insert(shortstatekey, pdu.event_id.clone());
			}
			state
		})
		.await;

	drop(cork);

	info!("Going through send_join response auth_chain");
	let cork = services.db.cork_and_flush();
	send_join_response
		.room_state
		.auth_chain
		.iter()
		.stream()
		.then(|pdu| {
			services
				.server_keys
				.validate_and_add_event_id_no_fetch(pdu, &room_version_id)
		})
		.ready_filter_map(Result::ok)
		.ready_for_each(|(event_id, value)| {
			trace!(%event_id, "Adding PDU as an outlier from send_join auth_chain");
			services.rooms.outlier.add_pdu_outlier(&event_id, &value);
		})
		.await;

	drop(cork);

	debug!("Running send_join auth check");
	let fetch_state = &state;
	let state_fetch = |k: StateEventType, s: StateKey| async move {
		let shortstatekey = services.rooms.short.get_shortstatekey(&k, &s).await.ok()?;

		let event_id = fetch_state.get(&shortstatekey)?;
		services.rooms.timeline.get_pdu(event_id).await.ok()
	};

	let auth_check = state_res::event_auth::auth_check(
		&state_res::RoomVersion::new(&room_version_id)?,
		&parsed_join_pdu,
		None, // TODO: third party invite
		|k, s| state_fetch(k.clone(), s.into()),
		&state_fetch(StateEventType::RoomCreate, "".into())
			.await
			.expect("create event is missing from send_join auth"),
	)
	.await
	.map_err(|e| err!(Request(Forbidden(warn!("Auth check failed: {e:?}")))))?;

	if !auth_check {
		return Err!(Request(Forbidden("Auth check failed")));
	}

	info!("Compressing state from send_join");
	let compressed: CompressedState = services
		.rooms
		.state_compressor
		.compress_state_events(state.iter().map(|(ssk, eid)| (ssk, eid.borrow())))
		.collect()
		.await;

	debug!("Saving compressed state");
	let HashSetCompressStateEvent {
		shortstatehash: statehash_before_join,
		added,
		removed,
	} = services
		.rooms
		.state_compressor
		.save_state(room_id, Arc::new(compressed))
		.await?;

	debug!("Forcing state for new room");
	services
		.rooms
		.state
		.force_state(room_id, statehash_before_join, added, removed, &state_lock)
		.await?;

	debug!("Updating joined counts for new room");
	services
		.rooms
		.state_cache
		.update_joined_count(room_id)
		.await;

	// We append to state before appending the pdu, so we don't have a moment in
	// time with the pdu without it's state. This is okay because append_pdu can't
	// fail.
	let statehash_after_join = services
		.rooms
		.state
		.append_to_state(&parsed_join_pdu, room_id)
		.await?;

	info!("Appending new room join event");
	services
		.rooms
		.timeline
		.append_pdu(
			&parsed_join_pdu,
			join_event,
			once(parsed_join_pdu.event_id.borrow()),
			&state_lock,
			room_id,
		)
		.await?;

	info!("Setting final room state for new room");
	// We set the room state after inserting the pdu, so that we never have a moment
	// in time where events in the current room state do not exist
	services
		.rooms
		.state
		.set_room_state(room_id, statehash_after_join, &state_lock);

	Ok(())
}

#[tracing::instrument(skip_all, fields(%sender_user, %room_id), name = "join_local", level = "info")]
async fn join_room_by_id_helper_local(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	servers: &[OwnedServerName],
	state_lock: RoomMutexGuard,
) -> Result {
	info!("Joining room locally");

	let (room_version, join_rules, is_invited) = join!(
		services.rooms.state.get_room_version(room_id),
		services.rooms.state_accessor.get_join_rules(room_id),
		services.rooms.state_cache.is_invited(sender_user, room_id)
	);

	let room_version = room_version?;
	let mut auth_user: Option<OwnedUserId> = None;
	if !is_invited && matches!(join_rules, JoinRule::Restricted(_) | JoinRule::KnockRestricted(_))
	{
		use RoomVersionId::*;
		if !matches!(room_version, V1 | V2 | V3 | V4 | V5 | V6 | V7) {
			// This is a restricted room, check if we can complete the join requirements
			// locally.
			let needs_auth_user =
				user_can_perform_restricted_join(services, sender_user, room_id, &room_version)
					.await;
			if needs_auth_user.is_ok_and(is_true!()) {
				// If there was an error or the value is false, we'll try joining over
				// federation. Since it's Ok(true), we can authorise this locally.
				// If we can't select a local user, this will remain None, the join will fail,
				// and we'll fall back to federation.
				auth_user = select_authorising_user(services, room_id, sender_user, &state_lock)
					.await
					.ok();
			}
		}
	}

	let content = RoomMemberEventContent {
		displayname: services.users.displayname(sender_user).await.ok(),
		avatar_url: services.users.avatar_url(sender_user).await.ok(),
		blurhash: services.users.blurhash(sender_user).await.ok(),
		reason: reason.clone(),
		join_authorized_via_users_server: auth_user,
		..RoomMemberEventContent::new(MembershipState::Join)
	};

	// Try normal join first
	let Err(error) = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(sender_user.to_string(), &content),
			sender_user,
			Some(room_id),
			&state_lock,
		)
		.await
	else {
		info!("Joined room locally");
		return Ok(());
	};

	if servers.is_empty() || servers.len() == 1 && services.globals.server_is_ours(&servers[0]) {
		return Err(error);
	}

	info!(
		?error,
		remote_servers = %servers.len(),
		"Could not join room locally, attempting remote join",
	);
	join_room_by_id_helper_remote(services, sender_user, room_id, reason, servers, state_lock)
		.await
}

async fn make_join_request(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	servers: &[OwnedServerName],
) -> Result<(federation::membership::prepare_join_event::v1::Response, OwnedServerName)> {
	let mut make_join_counter: usize = 1;

	for remote_server in servers {
		if services.globals.server_is_ours(remote_server) {
			continue;
		}
		info!(
			"Asking {remote_server} for make_join (attempt {make_join_counter}/{})",
			servers.len()
		);
		let make_join_response = services
			.sending
			.send_federation_request(
				remote_server,
				federation::membership::prepare_join_event::v1::Request {
					room_id: room_id.to_owned(),
					user_id: sender_user.to_owned(),
					ver: services.server.supported_room_versions().collect(),
				},
			)
			.await;

		trace!("make_join response: {:?}", make_join_response);
		make_join_counter = make_join_counter.saturating_add(1);

		match make_join_response {
			| Ok(response) => {
				info!("Received make_join response from {remote_server}");
				if let Err(e) = validate_remote_member_event_stub(
					&MembershipState::Join,
					sender_user,
					room_id,
					&to_canonical_object(&response.event)?,
				) {
					warn!("make_join response from {remote_server} failed validation: {e}");
					continue;
				}
				return Ok((response, remote_server.clone()));
			},
			| Err(e) => match e.kind() {
				| ErrorKind::UnableToAuthorizeJoin => {
					info!(
						"{remote_server} was unable to verify the joining user satisfied \
						 restricted join requirements: {e}. Will continue trying."
					);
				},
				| ErrorKind::UnableToGrantJoin => {
					info!(
						"{remote_server} believes the joining user satisfies restricted join \
						 rules, but is unable to authorise a join for us. Will continue trying."
					);
				},
				| ErrorKind::IncompatibleRoomVersion { room_version } => {
					warn!(
						"{remote_server} reports the room we are trying to join is \
						 v{room_version}, which we do not support."
					);
					return Err(e);
				},
				| ErrorKind::Forbidden { .. } => {
					warn!("{remote_server} refuses to let us join: {e}.");
					return Err(e);
				},
				| ErrorKind::NotFound => {
					info!(
						"{remote_server} does not know about {room_id}: {e}. Will continue \
						 trying."
					);
				},
				| _ => {
					info!("{remote_server} failed to make_join: {e}. Will continue trying.");
				},
			},
		}
	}
	info!("All {} servers were unable to assist in joining {room_id} :(", servers.len());
	Err!(BadServerResponse("No server available to assist in joining."))
}
