use std::{borrow::Borrow, time::Instant, vec};

use axum::extract::State;
use conduwuit::{
	Err, Event, Result, at, debug, err, info, trace,
	utils::stream::{BroadbandExt, IterStream, TryBroadbandExt},
	warn,
};
use conduwuit_service::Services;
use futures::{FutureExt, StreamExt, TryStreamExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, RoomId, ServerName, UserId,
	api::federation::membership::create_join_event,
	assign,
	events::{
		TimelineEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
	room_version_rules::RoomVersionRules,
};
use serde_json::value::to_raw_value;

use crate::{Ruma, server::utils::validate_any_membership_event};

/// helper method for /send_join v1 and v2
#[tracing::instrument(skip(services, pdu, omit_members), fields(room_id = room_id.as_str(), origin = origin.as_str()), level = "info")]
async fn create_join_event(
	services: &Services,
	origin: &ServerName,
	room_id: &RoomId,
	event_id: &EventId,
	room_version_rules: &RoomVersionRules,
	mut pdu: CanonicalJsonObject,
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
			room_id = %room_id,
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

	let sender = pdu
		.get("sender")
		.and_then(|v| v.as_str())
		.map(UserId::parse)
		.and_then(Result::ok)
		.expect("sender was already validated");

	let content: RoomMemberEventContent = serde_json::from_value(
		pdu.get("content")
			.ok_or_else(|| err!(Request(BadJson("Event missing content property"))))?
			.clone()
			.into(),
	)
	.map_err(|e| err!(Request(BadJson(warn!("Event content is empty or invalid: {e}")))))?;

	if let Some(authorising_user) = content.join_authorized_via_users_server {
		if !room_version_rules.authorization.restricted_join_rule {
			return Err!(Request(InvalidParam(
				"Room version does not support restricted rooms but \
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
		if !super::user_can_perform_restricted_join(services, &sender, room_id).await? {
			return Err!(Request(UnableToAuthorizeJoin(
				"Joining user did not pass restricted room's rules."
			)));
		}

		services
			.server_keys
			.hash_and_sign_event(&mut pdu, room_version_rules)
			.map_err(|e| {
				err!(Request(InvalidParam(warn!("Failed to sign send_join event: {e}"))))
			})?;
	}

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
		.handle_incoming_pdu(sender.server_name(), room_id, event_id, pdu.clone(), false)
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
			if omit_members && let Ok(e) = event_id.as_ref() {
				let pdu = services.rooms.timeline.get_pdu(e).await;
				if pdu.is_ok_and(|p| *p.kind() == TimelineEventType::RoomMember) {
					trace!("omitting member event {e:?} from returned state");
					// skip members
					return None;
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

	Ok(assign!(create_join_event::v2::RoomState::new(), {
		auth_chain,
		state,
		event: to_raw_value(&CanonicalJsonValue::Object(pdu)).ok(),
		members_omitted: omit_members,
		servers_in_room,
	}))
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
		.is_remote_server_forbidden(&body.identity)
	{
		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	let room_version = services.rooms.state.get_room_version(&body.room_id).await?;
	let create_event = services
		.rooms
		.state_accessor
		.get_room_create_event(&body.room_id)
		.await;
	let room_version_rules = room_version.rules().unwrap();

	let (value, membership, sender, target) = validate_any_membership_event(
		&services,
		&body.pdu,
		&room_version_rules,
		create_event.event_id.clone(),
		body.room_id.clone(),
		body.event_id.clone(),
	)
	.await?;
	if membership != MembershipState::Join {
		return Err!(Request(InvalidParam("Invalid membership (expected `join`)")));
	}
	if sender.server_name() != body.identity {
		return Err!(Request(InvalidParam("Sender belongs to a different server")));
	}
	if sender != target {
		return Err!(Request(InvalidParam("Sender does not match state key")));
	}

	let now = Instant::now();
	let room_state = create_join_event(
		&services,
		&body.identity,
		&body.room_id,
		&body.event_id,
		&room_version_rules,
		value,
		body.omit_members,
	)
	.boxed()
	.await?;
	info!(
		"Finished sending a join for {} in {} in {:?}",
		body.identity,
		&body.room_id,
		now.elapsed()
	);

	Ok(create_join_event::v2::Response::new(room_state))
}
