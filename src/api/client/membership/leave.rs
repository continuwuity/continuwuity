use std::collections::HashSet;

use axum::extract::State;
use conduwuit::{
	Err, Result, debug_info, debug_warn, err,
	matrix::pdu::{PduBuilder, gen_event_id},
	utils::{self, FutureBoolExt, future::ReadyEqExt},
	warn,
};
use futures::{FutureExt, StreamExt, TryFutureExt, pin_mut};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedServerName, RoomId, RoomVersionId, UserId,
	api::{
		client::membership::leave_room,
		federation::{self},
	},
	events::{
		StateEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
};
use service::Services;

use crate::Ruma;

/// # `POST /_matrix/client/v3/rooms/{roomId}/leave`
///
/// Tries to leave the sender user from a room.
///
/// - This should always work if the user is currently joined.
pub(crate) async fn leave_room_route(
	State(services): State<crate::State>,
	body: Ruma<leave_room::v3::Request>,
) -> Result<leave_room::v3::Response> {
	leave_room(&services, body.sender_user(), &body.room_id, body.reason.clone())
		.boxed()
		.await
		.map(|()| leave_room::v3::Response::new())
}

// Make a user leave all their joined rooms, rescinds knocks, forgets all rooms,
// and ignores errors
pub async fn leave_all_rooms(services: &Services, user_id: &UserId) {
	let rooms_joined = services
		.rooms
		.state_cache
		.rooms_joined(user_id)
		.map(ToOwned::to_owned);

	let rooms_invited = services
		.rooms
		.state_cache
		.rooms_invited(user_id)
		.map(|(r, _)| r);

	let rooms_knocked = services
		.rooms
		.state_cache
		.rooms_knocked(user_id)
		.map(|(r, _)| r);

	let all_rooms: Vec<_> = rooms_joined
		.chain(rooms_invited)
		.chain(rooms_knocked)
		.collect()
		.await;

	for room_id in all_rooms {
		// ignore errors
		if let Err(e) = leave_room(services, user_id, &room_id, None).boxed().await {
			warn!(%user_id, "Failed to leave {room_id} remotely: {e}");
		}

		services.rooms.state_cache.forget(&room_id, user_id);
	}
}

pub async fn leave_room(
	services: &Services,
	user_id: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
) -> Result {
	let default_member_content = RoomMemberEventContent {
		membership: MembershipState::Leave,
		reason: reason.clone(),
		join_authorized_via_users_server: None,
		is_direct: None,
		avatar_url: None,
		displayname: None,
		third_party_invite: None,
		blurhash: None,
	};

	let is_banned = services.rooms.metadata.is_banned(room_id);
	let is_disabled = services.rooms.metadata.is_disabled(room_id);

	pin_mut!(is_banned, is_disabled);
	if is_banned.or(is_disabled).await {
		// the room is banned/disabled, the room must be rejected locally since we
		// cant/dont want to federate with this server
		services
			.rooms
			.state_cache
			.update_membership(
				room_id,
				user_id,
				default_member_content,
				user_id,
				None,
				None,
				true,
			)
			.await?;

		return Ok(());
	}

	let dont_have_room = services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), room_id)
		.eq(&false);

	let not_knocked = services
		.rooms
		.state_cache
		.is_knocked(user_id, room_id)
		.eq(&false);

	// Ask a remote server if we don't have this room and are not knocking on it
	if dont_have_room.and(not_knocked).await {
		if let Err(e) = remote_leave_room(services, user_id, room_id, reason.clone())
			.boxed()
			.await
		{
			warn!(%user_id, "Failed to leave room {room_id} remotely: {e}");
			// Don't tell the client about this error
		}

		let last_state = services
			.rooms
			.state_cache
			.invite_state(user_id, room_id)
			.or_else(|_| services.rooms.state_cache.knock_state(user_id, room_id))
			.or_else(|_| services.rooms.state_cache.left_state(user_id, room_id))
			.await
			.ok();

		// We always drop the invite, we can't rely on other servers
		services
			.rooms
			.state_cache
			.update_membership(
				room_id,
				user_id,
				default_member_content,
				user_id,
				last_state,
				None,
				true,
			)
			.await?;
	} else {
		let state_lock = services.rooms.state.mutex.lock(room_id).await;

		let Ok(event) = services
			.rooms
			.state_accessor
			.room_state_get_content::<RoomMemberEventContent>(
				room_id,
				&StateEventType::RoomMember,
				user_id.as_str(),
			)
			.await
		else {
			debug_warn!(
				"Trying to leave a room you are not a member of, marking room as left locally."
			);

			return services
				.rooms
				.state_cache
				.update_membership(
					room_id,
					user_id,
					default_member_content,
					user_id,
					None,
					None,
					true,
				)
				.await;
		};

		services
			.rooms
			.timeline
			.build_and_append_pdu(
				PduBuilder::state(user_id.to_string(), &RoomMemberEventContent {
					membership: MembershipState::Leave,
					reason,
					join_authorized_via_users_server: None,
					is_direct: None,
					..event
				}),
				user_id,
				room_id,
				&state_lock,
			)
			.await?;
	}

	Ok(())
}

async fn remote_leave_room(
	services: &Services,
	user_id: &UserId,
	room_id: &RoomId,
	reason: Option<String>,
) -> Result<()> {
	let mut make_leave_response_and_server =
		Err!(BadServerResponse("No remote server available to assist in leaving {room_id}."));

	let mut servers: HashSet<OwnedServerName> = services
		.rooms
		.state_cache
		.servers_invite_via(room_id)
		.map(ToOwned::to_owned)
		.collect()
		.await;

	match services
		.rooms
		.state_cache
		.invite_state(user_id, room_id)
		.await
	{
		| Ok(invite_state) => {
			servers.extend(
				invite_state
					.iter()
					.filter_map(|event| event.get_field("sender").ok().flatten())
					.filter_map(|sender: &str| UserId::parse(sender).ok())
					.map(|user| user.server_name().to_owned()),
			);
		},
		| _ => {
			match services
				.rooms
				.state_cache
				.knock_state(user_id, room_id)
				.await
			{
				| Ok(knock_state) => {
					servers.extend(
						knock_state
							.iter()
							.filter_map(|event| event.get_field("sender").ok().flatten())
							.filter_map(|sender: &str| UserId::parse(sender).ok())
							.filter_map(|sender| {
								if !services.globals.user_is_local(sender) {
									Some(sender.server_name().to_owned())
								} else {
									None
								}
							}),
					);
				},
				| _ => {},
			}
		},
	}

	if let Some(room_id_server_name) = room_id.server_name() {
		servers.insert(room_id_server_name.to_owned());
	}

	debug_info!("servers in remote_leave_room: {servers:?}");

	for remote_server in servers {
		let make_leave_response = services
			.sending
			.send_federation_request(
				&remote_server,
				federation::membership::prepare_leave_event::v1::Request {
					room_id: room_id.to_owned(),
					user_id: user_id.to_owned(),
				},
			)
			.await;

		make_leave_response_and_server = make_leave_response.map(|r| (r, remote_server));

		if make_leave_response_and_server.is_ok() {
			break;
		}
	}

	let (make_leave_response, remote_server) = make_leave_response_and_server?;

	let Some(room_version_id) = make_leave_response.room_version else {
		return Err!(BadServerResponse(warn!(
			"No room version was returned by {remote_server} for {room_id}, room version is \
			 likely not supported by conduwuit"
		)));
	};

	if !services.server.supported_room_version(&room_version_id) {
		return Err!(BadServerResponse(warn!(
			"Remote room version {room_version_id} for {room_id} is not supported by conduwuit",
		)));
	}

	let mut leave_event_stub = serde_json::from_str::<CanonicalJsonObject>(
		make_leave_response.event.get(),
	)
	.map_err(|e| {
		err!(BadServerResponse(warn!(
			"Invalid make_leave event json received from {remote_server} for {room_id}: {e:?}"
		)))
	})?;

	// TODO: Is origin needed?
	leave_event_stub.insert(
		"origin".to_owned(),
		CanonicalJsonValue::String(services.globals.server_name().as_str().to_owned()),
	);
	leave_event_stub.insert(
		"origin_server_ts".to_owned(),
		CanonicalJsonValue::Integer(
			utils::millis_since_unix_epoch()
				.try_into()
				.expect("Timestamp is valid js_int value"),
		),
	);
	// Inject the reason key into the event content dict if it exists
	if let Some(reason) = reason {
		if let Some(CanonicalJsonValue::Object(content)) = leave_event_stub.get_mut("content") {
			content.insert("reason".to_owned(), CanonicalJsonValue::String(reason));
		}
	}

	// room v3 and above removed the "event_id" field from remote PDU format
	match room_version_id {
		| RoomVersionId::V1 | RoomVersionId::V2 => {},
		| _ => {
			leave_event_stub.remove("event_id");
		},
	}

	// In order to create a compatible ref hash (EventID) the `hashes` field needs
	// to be present
	services
		.server_keys
		.hash_and_sign_event(&mut leave_event_stub, &room_version_id)?;

	// Generate event id
	let event_id = gen_event_id(&leave_event_stub, &room_version_id)?;

	// Add event_id back
	leave_event_stub
		.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.clone().into()));

	// It has enough fields to be called a proper event now
	let leave_event = leave_event_stub;

	services
		.sending
		.send_federation_request(
			&remote_server,
			federation::membership::create_leave_event::v2::Request {
				room_id: room_id.to_owned(),
				event_id,
				pdu: services
					.sending
					.convert_to_outgoing_federation_event(leave_event.clone())
					.await,
			},
		)
		.await?;

	Ok(())
}
