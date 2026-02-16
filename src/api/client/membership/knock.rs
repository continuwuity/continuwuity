use std::{borrow::Borrow, collections::HashMap, iter::once, sync::Arc};

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Result, debug, debug_info, debug_warn, err, info,
	matrix::{
		event::gen_event_id,
		pdu::{PduBuilder, PduEvent},
	},
	result::FlatOk,
	trace,
	utils::{self, shuffle, stream::IterStream, to_canonical_object},
	warn,
};
use futures::{FutureExt, StreamExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, OwnedRoomId, OwnedServerName, RoomId,
	RoomVersionId, UserId,
	api::{
		client::knock::knock_room,
		federation::{self},
	},
	canonical_json::to_canonical_value,
	events::{
		StateEventType,
		room::{
			join_rules::{AllowRule, JoinRule},
			member::{MembershipState, RoomMemberEventContent},
		},
	},
};
use service::{
	Services,
	rooms::{
		state::RoomMutexGuard,
		state_compressor::{CompressedState, HashSetCompressStateEvent},
	},
};

use super::{banned_room_check, join::join_room_by_id_helper, validate_remote_member_event_stub};
use crate::Ruma;

/// # `POST /_matrix/client/*/knock/{roomIdOrAlias}`
///
/// Tries to knock the room to ask permission to join for the sender user.
#[tracing::instrument(skip_all, fields(%client), name = "knock", level = "info")]
pub(crate) async fn knock_room_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<knock_room::v3::Request>,
) -> Result<knock_room::v3::Response> {
	let sender_user = body.sender_user();
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
			.await?;

			let mut servers = body.via.clone();
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
		return Ok(knock_room::v3::Response { room_id: room_id.into() });
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

	// For knock_restricted rooms, check if the user meets the restricted conditions
	// If they do, attempt to join instead of knock
	// This is not mentioned in the spec, but should be allowable (we're allowed to
	// auto-join invites to knocked rooms)
	let join_rule = services.rooms.state_accessor.get_join_rules(room_id).await;

	if let JoinRule::KnockRestricted(restricted) = &join_rule {
		let restriction_rooms: Vec<_> = restricted
			.allow
			.iter()
			.filter_map(|a| match a {
				| AllowRule::RoomMembership(r) => Some(&r.room_id),
				| _ => None,
			})
			.collect();

		// Check if the user is in any of the allowed rooms
		let mut user_meets_restrictions = false;
		for restriction_room_id in &restriction_rooms {
			if services
				.rooms
				.state_cache
				.is_joined(sender_user, restriction_room_id)
				.await
			{
				user_meets_restrictions = true;
				break;
			}
		}

		// If the user meets the restrictions, try joining instead
		if user_meets_restrictions {
			debug_info!(
				"{sender_user} meets the restricted criteria in knock_restricted room \
				 {room_id}, attempting to join instead of knock"
			);
			// For this case, we need to drop the state lock and get a new one in
			// join_room_by_id_helper We need to release the lock here and let
			// join_room_by_id_helper acquire it again
			drop(state_lock);
			match join_room_by_id_helper(
				services,
				sender_user,
				room_id,
				reason.clone(),
				servers,
				&None,
			)
			.await
			{
				| Ok(_) => return Ok(knock_room::v3::Response::new(room_id.to_owned())),
				| Err(e) => {
					debug_warn!(
						"Failed to convert knock to join for {sender_user} in {room_id}: {e:?}"
					);
					// Get a new state lock for the remaining knock logic
					let new_state_lock = services.rooms.state.mutex.lock(room_id).await;

					let server_in_room = services
						.rooms
						.state_cache
						.server_in_room(services.globals.server_name(), room_id)
						.await;

					let local_knock = server_in_room
						|| servers.is_empty()
						|| (servers.len() == 1 && services.globals.server_is_ours(&servers[0]));

					if local_knock {
						knock_room_helper_local(
							services,
							sender_user,
							room_id,
							reason,
							servers,
							new_state_lock,
						)
						.boxed()
						.await?;
					} else {
						knock_room_helper_remote(
							services,
							sender_user,
							room_id,
							reason,
							servers,
							new_state_lock,
						)
						.boxed()
						.await?;
					}

					return Ok(knock_room::v3::Response::new(room_id.to_owned()));
				},
			}
		}
	} else if !matches!(join_rule, JoinRule::Knock | JoinRule::KnockRestricted(_)) {
		debug_warn!(
			"{sender_user} attempted to knock on room {room_id} but its join rule is \
			 {join_rule:?}, not knock or knock_restricted"
		);
	}

	let server_in_room = services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), room_id)
		.await;

	let local_knock = server_in_room
		|| servers.is_empty()
		|| (servers.len() == 1 && services.globals.server_is_ours(&servers[0]));

	if local_knock {
		knock_room_helper_local(services, sender_user, room_id, reason, servers, state_lock)
			.boxed()
			.await?;
	} else {
		knock_room_helper_remote(services, sender_user, room_id, reason, servers, state_lock)
			.boxed()
			.await?;
	}

	Ok(knock_room::v3::Response::new(room_id.to_owned()))
}

async fn knock_room_helper_local(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	servers: &[OwnedServerName],
	state_lock: RoomMutexGuard,
) -> Result {
	debug_info!("We can knock locally");

	let room_version_id = services.rooms.state.get_room_version(room_id).await?;

	if matches!(
		room_version_id,
		RoomVersionId::V1
			| RoomVersionId::V2
			| RoomVersionId::V3
			| RoomVersionId::V4
			| RoomVersionId::V5
			| RoomVersionId::V6
	) {
		return Err!(Request(Forbidden("This room does not support knocking.")));
	}

	let content = RoomMemberEventContent {
		displayname: services.users.displayname(sender_user).await.ok(),
		avatar_url: services.users.avatar_url(sender_user).await.ok(),
		blurhash: services.users.blurhash(sender_user).await.ok(),
		reason: reason.clone(),
		..RoomMemberEventContent::new(MembershipState::Knock)
	};

	// Try normal knock first
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
		return Ok(());
	};

	if servers.is_empty() || (servers.len() == 1 && services.globals.server_is_ours(&servers[0]))
	{
		return Err(error);
	}

	warn!("We couldn't do the knock locally, maybe federation can help to satisfy the knock");

	let (make_knock_response, remote_server) =
		make_knock_request(services, sender_user, room_id, servers).await?;

	info!("make_knock finished");

	let room_version_id = make_knock_response.room_version;

	if !services.server.supported_room_version(&room_version_id) {
		return Err!(BadServerResponse(
			"Remote room version {room_version_id} is not supported by conduwuit"
		));
	}

	let mut knock_event_stub = serde_json::from_str::<CanonicalJsonObject>(
		make_knock_response.event.get(),
	)
	.map_err(|e| {
		err!(BadServerResponse("Invalid make_knock event json received from server: {e:?}"))
	})?;

	validate_remote_member_event_stub(
		&MembershipState::Knock,
		sender_user,
		room_id,
		&knock_event_stub,
	)?;

	knock_event_stub.insert(
		"origin".to_owned(),
		CanonicalJsonValue::String(services.globals.server_name().as_str().to_owned()),
	);
	knock_event_stub.insert(
		"origin_server_ts".to_owned(),
		CanonicalJsonValue::Integer(
			utils::millis_since_unix_epoch()
				.try_into()
				.expect("Timestamp is valid js_int value"),
		),
	);
	knock_event_stub.insert(
		"content".to_owned(),
		to_canonical_value(RoomMemberEventContent {
			displayname: services.users.displayname(sender_user).await.ok(),
			avatar_url: services.users.avatar_url(sender_user).await.ok(),
			blurhash: services.users.blurhash(sender_user).await.ok(),
			reason,
			..RoomMemberEventContent::new(MembershipState::Knock)
		})
		.expect("event is valid, we just created it"),
	);

	// In order to create a compatible ref hash (EventID) the `hashes` field needs
	// to be present
	services
		.server_keys
		.hash_and_sign_event(&mut knock_event_stub, &room_version_id)?;

	// Generate event id
	let event_id = gen_event_id(&knock_event_stub, &room_version_id)?;

	// Add event_id
	knock_event_stub
		.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.clone().into()));

	// It has enough fields to be called a proper event now
	let knock_event = knock_event_stub;

	info!("Asking {remote_server} for send_knock in room {room_id}");
	let send_knock_request = federation::knock::send_knock::v1::Request {
		room_id: room_id.to_owned(),
		event_id: event_id.clone(),
		pdu: services
			.sending
			.convert_to_outgoing_federation_event(knock_event.clone())
			.await,
	};

	services
		.sending
		.send_federation_request(&remote_server, send_knock_request)
		.await?;

	info!("send_knock finished");

	services
		.rooms
		.short
		.get_or_create_shortroomid(room_id)
		.await;

	info!("Parsing knock event");

	let parsed_knock_pdu = PduEvent::from_id_val(&event_id, knock_event.clone())
		.map_err(|e| err!(BadServerResponse("Invalid knock event PDU: {e:?}")))?;

	info!("Updating membership locally to knock state with provided stripped state events");
	// TODO: this call does not appear to do anything because `update_membership`
	// doesn't call `mark_as_knock`. investigate further, ideally with the aim of
	// removing this call entirely -- Ginger thinks `update_membership` should only
	// be called from `force_state` and `append_pdu`.
	services
		.rooms
		.state_cache
		.update_membership(room_id, sender_user, &parsed_knock_pdu, false)
		.await?;

	info!("Appending room knock event locally");
	services
		.rooms
		.timeline
		.append_pdu(
			&parsed_knock_pdu,
			knock_event,
			once(parsed_knock_pdu.event_id.borrow()),
			&state_lock,
			room_id,
		)
		.await?;

	Ok(())
}

async fn knock_room_helper_remote(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
	servers: &[OwnedServerName],
	state_lock: RoomMutexGuard,
) -> Result {
	info!("Knocking {room_id} over federation.");

	let (make_knock_response, remote_server) =
		make_knock_request(services, sender_user, room_id, servers).await?;

	info!("make_knock finished");

	let room_version_id = make_knock_response.room_version;

	if !services.server.supported_room_version(&room_version_id) {
		return Err!(BadServerResponse(
			"Remote room version {room_version_id} is not supported by conduwuit"
		));
	}

	let mut knock_event_stub: CanonicalJsonObject =
		serde_json::from_str(make_knock_response.event.get()).map_err(|e| {
			err!(BadServerResponse("Invalid make_knock event json received from server: {e:?}"))
		})?;

	knock_event_stub.insert(
		"origin".to_owned(),
		CanonicalJsonValue::String(services.globals.server_name().as_str().to_owned()),
	);
	knock_event_stub.insert(
		"origin_server_ts".to_owned(),
		CanonicalJsonValue::Integer(
			utils::millis_since_unix_epoch()
				.try_into()
				.expect("Timestamp is valid js_int value"),
		),
	);
	knock_event_stub.insert(
		"content".to_owned(),
		to_canonical_value(RoomMemberEventContent {
			displayname: services.users.displayname(sender_user).await.ok(),
			avatar_url: services.users.avatar_url(sender_user).await.ok(),
			blurhash: services.users.blurhash(sender_user).await.ok(),
			reason,
			..RoomMemberEventContent::new(MembershipState::Knock)
		})
		.expect("event is valid, we just created it"),
	);

	// In order to create a compatible ref hash (EventID) the `hashes` field needs
	// to be present
	services
		.server_keys
		.hash_and_sign_event(&mut knock_event_stub, &room_version_id)?;

	// Generate event id
	let event_id = gen_event_id(&knock_event_stub, &room_version_id)?;

	// Add event_id
	knock_event_stub
		.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.clone().into()));

	// It has enough fields to be called a proper event now
	let knock_event = knock_event_stub;

	info!("Asking {remote_server} for send_knock in room {room_id}");
	let send_knock_request = federation::knock::send_knock::v1::Request {
		room_id: room_id.to_owned(),
		event_id: event_id.clone(),
		pdu: services
			.sending
			.convert_to_outgoing_federation_event(knock_event.clone())
			.await,
	};

	let send_knock_response = services
		.sending
		.send_federation_request(&remote_server, send_knock_request)
		.await?;

	info!("send_knock finished");

	services
		.rooms
		.short
		.get_or_create_shortroomid(room_id)
		.await;

	info!("Parsing knock event");
	let parsed_knock_pdu = PduEvent::from_id_val(&event_id, knock_event.clone())
		.map_err(|e| err!(BadServerResponse("Invalid knock event PDU: {e:?}")))?;

	info!("Going through send_knock response knock state events");
	let state = send_knock_response
		.knock_room_state
		.iter()
		.map(|event| serde_json::from_str::<CanonicalJsonObject>(event.clone().into_json().get()))
		.filter_map(Result::ok);

	let mut state_map: HashMap<u64, OwnedEventId> = HashMap::new();

	for event in state {
		let Some(state_key) = event.get("state_key") else {
			debug_warn!("send_knock stripped state event missing state_key: {event:?}");
			continue;
		};
		let Some(event_type) = event.get("type") else {
			debug_warn!("send_knock stripped state event missing event type: {event:?}");
			continue;
		};

		let Ok(state_key) = serde_json::from_value::<String>(state_key.clone().into()) else {
			debug_warn!("send_knock stripped state event has invalid state_key: {event:?}");
			continue;
		};
		let Ok(event_type) = serde_json::from_value::<StateEventType>(event_type.clone().into())
		else {
			debug_warn!("send_knock stripped state event has invalid event type: {event:?}");
			continue;
		};

		let event_id = gen_event_id(&event, &room_version_id)?;
		let shortstatekey = services
			.rooms
			.short
			.get_or_create_shortstatekey(&event_type, &state_key)
			.await;

		services.rooms.outlier.add_pdu_outlier(&event_id, &event);
		state_map.insert(shortstatekey, event_id.clone());
	}

	info!("Compressing state from send_knock");
	let compressed: CompressedState = services
		.rooms
		.state_compressor
		.compress_state_events(state_map.iter().map(|(ssk, eid)| (ssk, eid.borrow())))
		.collect()
		.await;

	debug!("Saving compressed state");
	let HashSetCompressStateEvent {
		shortstatehash: statehash_before_knock,
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
		.force_state(room_id, statehash_before_knock, added, removed, &state_lock)
		.await?;

	let statehash_after_knock = services
		.rooms
		.state
		.append_to_state(&parsed_knock_pdu, room_id)
		.await?;

	info!("Updating membership locally to knock state with provided stripped state events");
	// TODO: see TODO on the other call to `update_membership`
	services
		.rooms
		.state_cache
		.update_membership(room_id, sender_user, &parsed_knock_pdu, false)
		.await?;

	info!("Appending room knock event locally");
	services
		.rooms
		.timeline
		.append_pdu(
			&parsed_knock_pdu,
			knock_event,
			once(parsed_knock_pdu.event_id.borrow()),
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
		.set_room_state(room_id, statehash_after_knock, &state_lock);

	Ok(())
}

async fn make_knock_request(
	services: &Services,
	sender_user: &UserId,
	room_id: &RoomId,
	servers: &[OwnedServerName],
) -> Result<(federation::knock::create_knock_event_template::v1::Response, OwnedServerName)> {
	let mut make_knock_response_and_server =
		Err!(BadServerResponse("No server available to assist in knocking."));

	let mut make_knock_counter: usize = 0;

	for remote_server in servers {
		if services.globals.server_is_ours(remote_server) {
			continue;
		}

		info!("Asking {remote_server} for make_knock ({make_knock_counter})");

		let make_knock_response = services
			.sending
			.send_federation_request(
				remote_server,
				federation::knock::create_knock_event_template::v1::Request {
					room_id: room_id.to_owned(),
					user_id: sender_user.to_owned(),
					ver: services.server.supported_room_versions().collect(),
				},
			)
			.await;

		trace!("make_knock response: {make_knock_response:?}");
		make_knock_counter = make_knock_counter.saturating_add(1);
		if let Ok(r) = &make_knock_response {
			if let Err(e) = validate_remote_member_event_stub(
				&MembershipState::Knock,
				sender_user,
				room_id,
				&to_canonical_object(&r.event)?,
			) {
				warn!("make_knock response from {remote_server} failed validation: {e}");
				continue;
			}
		}

		make_knock_response_and_server = make_knock_response.map(|r| (r, remote_server.clone()));

		if make_knock_response_and_server.is_ok() {
			break;
		}

		if make_knock_counter > 40 {
			warn!(
				"50 servers failed to provide valid make_knock response, assuming no server can \
				 assist in knocking."
			);
			make_knock_response_and_server =
				Err!(BadServerResponse("No server available to assist in knocking."));

			return make_knock_response_and_server;
		}
	}

	make_knock_response_and_server
}
