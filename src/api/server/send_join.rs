#![allow(deprecated)]

use std::{borrow::Borrow, time::Instant, vec};

use axum::extract::State;
use conduwuit::{
	Err, Event, Result, at, debug, err, info,
	matrix::event::gen_event_id_canonical_json,
	trace,
	utils::stream::{BroadbandExt, IterStream, TryBroadbandExt},
	warn,
};
use conduwuit_service::Services;
use futures::{FutureExt, StreamExt, TryStreamExt};
use ruma::{
	CanonicalJsonValue, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, ServerName,
	api::federation::membership::create_join_event,
	events::{
		StateEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
};
use serde_json::value::{RawValue as RawJsonValue, to_raw_value};

use crate::Ruma;

/// helper method for /send_join v1 and v2
#[tracing::instrument(skip(services, pdu, omit_members), fields(room_id = room_id.as_str(), origin = origin.as_str()), level = "info")]
async fn create_join_event(
	services: &Services,
	origin: &ServerName,
	room_id: &RoomId,
	pdu: &RawJsonValue,
	omit_members: bool,
) -> Result<create_join_event::v2::RoomState> {
	if !services.rooms.metadata.exists(room_id).await {
		return Err!(Request(NotFound("Room is unknown to this server.")));
	}
	if !services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), room_id)
		.await
	{
		info!(
			origin = origin.as_str(),
			"Refusing to serve send_join for room we aren't participating in"
		);
		return Err!(Request(NotFound("This server is not participating in that room.")));
	}

	// ACL check origin server
	services
		.rooms
		.event_handler
		.acl_check(origin, room_id)
		.await?;

	// We need to return the state prior to joining, let's keep a reference to that
	// here
	let shortstatehash = services
		.rooms
		.state
		.get_room_shortstatehash(room_id)
		.await
		.map_err(|e| err!(Request(NotFound(error!("Room has no state: {e}")))))?;

	// We do not add the event_id field to the pdu here because of signature and
	// hashes checks
	trace!("Getting room version");
	let room_version_id = services.rooms.state.get_room_version(room_id).await?;

	trace!("Generating event ID and converting to canonical json");
	let Ok((event_id, mut value)) = gen_event_id_canonical_json(pdu, &room_version_id) else {
		// Event could not be converted to canonical json
		return Err!(Request(BadJson("Could not convert event to canonical json.")));
	};

	let event_room_id: OwnedRoomId = serde_json::from_value(
		value
			.get("room_id")
			.ok_or_else(|| err!(Request(BadJson("Event missing room_id property."))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson(warn!("room_id field is not a valid room ID: {e}")))))?;

	if event_room_id != room_id {
		return Err!(Request(BadJson("Event room_id does not match request path room ID.")));
	}

	let event_type: StateEventType = serde_json::from_value(
		value
			.get("type")
			.ok_or_else(|| err!(Request(BadJson("Event missing type property."))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson(warn!("Event has invalid state event type: {e}")))))?;

	if event_type != StateEventType::RoomMember {
		return Err!(Request(BadJson(
			"Not allowed to send non-membership state event to join endpoint."
		)));
	}

	let content: RoomMemberEventContent = serde_json::from_value(
		value
			.get("content")
			.ok_or_else(|| err!(Request(BadJson("Event missing content property"))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson(warn!("Event content is empty or invalid: {e}")))))?;

	if content.membership != MembershipState::Join {
		return Err!(Request(BadJson(
			"Not allowed to send a non-join membership event to join endpoint."
		)));
	}

	let sender: OwnedUserId = serde_json::from_value(
		value
			.get("sender")
			.ok_or_else(|| err!(Request(BadJson("Event missing sender property."))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson(warn!("sender property is not a valid user ID: {e}")))))?;

	// check if origin server is trying to send for another server
	if sender.server_name() != origin {
		return Err!(Request(Forbidden("Not allowed to join on behalf of another server.")));
	}

	let state_key: OwnedUserId = serde_json::from_value(
		value
			.get("state_key")
			.ok_or_else(|| err!(Request(BadJson("Event missing state_key property."))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson(warn!("State key is not a valid user ID: {e}")))))?;

	if state_key != sender {
		return Err!(Request(BadJson("State key does not match sender user.")));
	}

	if let Some(authorising_user) = content.join_authorized_via_users_server {
		use ruma::RoomVersionId::*;

		if matches!(room_version_id, V1 | V2 | V3 | V4 | V5 | V6 | V7) {
			return Err!(Request(InvalidParam(
				"Room version {room_version_id} does not support restricted rooms but \
				 join_authorised_via_users_server ({authorising_user}) was found in the event."
			)));
		}

		if !services.globals.user_is_local(&authorising_user) {
			return Err!(Request(InvalidParam(
				"Cannot authorise membership event through {authorising_user} as they do not \
				 belong to this homeserver"
			)));
		}

		if !services
			.rooms
			.state_cache
			.is_joined(&authorising_user, room_id)
			.await
		{
			return Err!(Request(InvalidParam(
				"Authorising user {authorising_user} is not in the room you are trying to join, \
				 they cannot authorise your join."
			)));
		}

		if !super::user_can_perform_restricted_join(
			services,
			&state_key,
			room_id,
			&room_version_id,
		)
		.await?
		{
			return Err!(Request(UnableToAuthorizeJoin(
				"Joining user did not pass restricted room's rules."
			)));
		}
	}

	trace!("Signing send_join event");
	services
		.server_keys
		.hash_and_sign_event(&mut value, &room_version_id)
		.map_err(|e| err!(Request(InvalidParam(warn!("Failed to sign send_join event: {e}")))))?;

	let mutex_lock = services
		.rooms
		.event_handler
		.mutex_federation
		.lock(room_id)
		.await;

	trace!("Acquired send_join mutex, persisting join event");
	let pdu_id = services
		.rooms
		.event_handler
		.handle_incoming_pdu(sender.server_name(), room_id, &event_id, value.clone(), true)
		.boxed()
		.await?
		.ok_or_else(|| err!(Request(InvalidParam("Could not accept as timeline event."))))?;

	drop(mutex_lock);
	trace!("Fetching current state IDs");
	let state_ids: Vec<OwnedEventId> = services
		.rooms
		.state_accessor
		.state_full_ids(shortstatehash)
		.map(at!(1))
		.collect()
		.await;

	trace!(%omit_members, "Constructing current state");
	let state = state_ids
		.iter()
		.try_stream()
		.broad_filter_map(|event_id| async move {
			if omit_members {
				if let Ok(e) = event_id.as_ref() {
					let pdu = services.rooms.timeline.get_pdu(e).await;
					if pdu.is_ok_and(|p| p.kind().to_cow_str() == "m.room.member") {
						trace!("omitting member event {e:?} from returned state");
						// skip members
						return None;
					}
				}
			}
			Some(event_id)
		})
		.broad_and_then(|event_id| services.rooms.timeline.get_pdu_json(event_id))
		.broad_and_then(|pdu| {
			services
				.sending
				.convert_to_outgoing_federation_event(pdu)
				.map(Ok)
		})
		.try_collect()
		.boxed()
		.await?;

	let starting_events = state_ids.iter().map(Borrow::borrow);
	trace!("Constructing auth chain");
	let auth_chain = services
		.rooms
		.auth_chain
		.event_ids_iter(room_id, starting_events)
		.broad_and_then(|event_id| async move {
			services.rooms.timeline.get_pdu_json(&event_id).await
		})
		.broad_and_then(|pdu| {
			services
				.sending
				.convert_to_outgoing_federation_event(pdu)
				.map(Ok)
		})
		.try_collect()
		.boxed()
		.await?;
	info!(fast_join = %omit_members, "Sending join event to other servers");
	services.sending.send_pdu_room(room_id, &pdu_id).await?;
	debug!("Finished sending join event");
	let servers_in_room: Option<Vec<_>> = if !omit_members {
		None
	} else {
		trace!("Fetching list of servers in room");
		let servers: Vec<String> = services
			.rooms
			.state_cache
			.room_servers(room_id)
			.map(|sn| sn.as_str().to_owned())
			.collect()
			.await;
		// If there's no servers, just add us
		let servers = if servers.is_empty() {
			warn!("Failed to find any servers, adding our own server name as a last resort");
			vec![services.globals.server_name().to_string()]
		} else {
			trace!("Found {} servers in room", servers.len());
			servers
		};
		Some(servers)
	};
	debug!("Returning send_join data");
	Ok(create_join_event::v2::RoomState {
		auth_chain,
		state,
		event: to_raw_value(&CanonicalJsonValue::Object(value)).ok(),
		members_omitted: omit_members,
		servers_in_room,
	})
}

/// # `PUT /_matrix/federation/v1/send_join/{roomId}/{eventId}`
///
/// Submits a signed join event.
pub(crate) async fn create_join_event_v1_route(
	State(services): State<crate::State>,
	body: Ruma<create_join_event::v1::Request>,
) -> Result<create_join_event::v1::Response> {
	if services
		.moderation
		.is_remote_server_forbidden(body.origin())
	{
		warn!(
			"Server {} tried joining room ID {} through us who has a server name that is \
			 globally forbidden. Rejecting.",
			body.origin(),
			&body.room_id,
		);
		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			warn!(
				"Server {} tried joining room ID {} through us which has a server name that is \
				 globally forbidden. Rejecting.",
				body.origin(),
				&body.room_id,
			);
			return Err!(Request(Forbidden(warn!(
				"Room ID server name {server} is banned on this homeserver."
			))));
		}
	}

	let now = Instant::now();
	let room_state = create_join_event(&services, body.origin(), &body.room_id, &body.pdu, false)
		.boxed()
		.await?;
	let transformed = create_join_event::v1::RoomState {
		auth_chain: room_state.auth_chain,
		state: room_state.state,
		event: room_state.event,
	};
	info!(
		"Finished sending a join for {} in {} in {:?}",
		body.origin(),
		&body.room_id,
		now.elapsed()
	);

	Ok(create_join_event::v1::Response { room_state: transformed })
}

/// # `PUT /_matrix/federation/v2/send_join/{roomId}/{eventId}`
///
/// Submits a signed join event.
pub(crate) async fn create_join_event_v2_route(
	State(services): State<crate::State>,
	body: Ruma<create_join_event::v2::Request>,
) -> Result<create_join_event::v2::Response> {
	if services
		.moderation
		.is_remote_server_forbidden(body.origin())
	{
		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	if let Some(server) = body.room_id.server_name() {
		if services.moderation.is_remote_server_forbidden(server) {
			warn!(
				"Server {} tried joining room ID {} through us which has a server name that is \
				 globally forbidden. Rejecting.",
				body.origin(),
				&body.room_id,
			);
			return Err!(Request(Forbidden(warn!(
				"Room ID server name {server} is banned on this homeserver."
			))));
		}
	}

	let now = Instant::now();
	let room_state =
		create_join_event(&services, body.origin(), &body.room_id, &body.pdu, body.omit_members)
			.boxed()
			.await?;
	info!(
		"Finished sending a join for {} in {} in {:?}",
		body.origin(),
		&body.room_id,
		now.elapsed()
	);

	Ok(create_join_event::v2::Response { room_state })
}
