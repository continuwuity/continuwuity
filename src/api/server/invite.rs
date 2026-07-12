use std::collections::{HashMap, hash_map::Entry};

use axum::extract::State;
use base64::{Engine as _, engine::general_purpose};
use conduwuit::{
	Err, Error, EventTypeExt, PduEvent, Result, debug, err, error,
	matrix::{Event, StateKey},
	result::FlatOk,
	state_res, trace,
	utils::hash::sha256,
	warn,
};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, OwnedRoomId, OwnedUserId, ServerName,
	UserId,
	api::{
		error::{ErrorKind, IncompatibleRoomVersionErrorData},
		federation::membership::{RawStrippedState, create_invite},
	},
	events::{StateEventType, room::member::MembershipState},
	room_version_rules::RoomVersionRules,
};
use serde::Deserialize;

use crate::{Ruma, client_ip::ClientIp, server::utils::validate_any_membership_event};

/// # `PUT /_matrix/federation/v2/invite/{roomId}/{eventId}`
///
/// Invites a remote user to a room.
#[tracing::instrument(skip_all, name = "invite", level = "info")]
pub(crate) async fn create_invite_route(
	State(services): State<crate::State>,
	body: Ruma<create_invite::v2::Request>,
) -> Result<create_invite::v2::Response> {
	if !services.server.supported_room_version(&body.room_version) {
		return Err(Error::BadRequest(
			ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData::new(
				body.room_version.clone(),
			)),
			"This server does not support that room version",
		));
	}
	let room_version_rules = body.room_version.rules().unwrap();

	if services
		.moderation
		.is_remote_server_forbidden(&body.identity)
	{
		warn!(
			"Received federated/remote invite from banned server {} for room ID {}. Rejecting.",
			body.identity, body.room_id
		);

		return Err!(Request(Forbidden("Federation denied with {}", body.identity)));
	}

	// First, validate the invite room state, so we can compare with the create
	// event.
	debug!(
		event_id=%body.event_id,
		room_id=%body.room_id,
		room_version=?body.room_version,
		via=?body.via,
		"Validating invite room state for invite request"
	);
	let (create_event_id, state) = validate_invite_state(
		&services,
		&body.invite_room_state,
		&room_version_rules,
		body.room_id.clone(),
	)
	.await?;
	let create_event_json = state
		.get(&StateEventType::RoomCreate.with_state_key(""))
		.expect("must have create event in invite state by this point");

	// We can now perform the banned remote server check with the create event.
	// N.B. this checks the sender field, which is technically incorrect for rooms
	// v10 and below. This usually isn't the case though so sue me
	let creator = create_event_json
		.get("sender")
		.and_then(|v| v.as_str())
		.map(UserId::parse)
		.flat_ok()
		.expect("must have valid sender in create event");
	if services
		.moderation
		.is_remote_server_forbidden(creator.server_name())
	{
		return Err!(Request(Forbidden("Server is banned on this homeserver.")));
	}

	// And then we can validate the member event itself
	let (mut signed_event, sender_user, recipient_user) = validate_invite_membership_event(
		&services,
		&body.event,
		&room_version_rules,
		&body.identity,
		create_event_id.clone(),
		body.room_id.clone(),
		body.event_id.clone(),
	)
	.await?;

	if services.rooms.metadata.is_banned(&body.room_id).await
		&& !services.users.is_admin(&recipient_user).await
	{
		return Err!(Request(Forbidden("That room is banned on this homeserver.")));
	}

	if services.config.block_non_admin_invites && !services.users.is_admin(&recipient_user).await
	{
		return Err!(Request(Forbidden("This server does not allow room invites.")));
	}

	if let Err(e) = services
		.antispam
		.user_may_invite(sender_user.clone(), recipient_user.clone(), body.room_id.clone())
		.await
	{
		warn!("Antispam rejected invite: {e:?}");
		return Err!(Request(Forbidden("Invite rejected by antispam service.")));
	}

	// If we're already in the room, ensure that neither the origin nor ourselves
	// are ACL'd.
	let resident = services
		.rooms
		.state_cache
		.server_in_room(services.globals.server_name(), &body.room_id)
		.await;
	if resident {
		services
			.rooms
			.event_handler
			.acl_check(&body.identity, &body.room_id)
			.await?;
		services
			.rooms
			.event_handler
			.acl_check(recipient_user.server_name(), &body.room_id)
			.await
			.map_err(|_| err!(Request(Forbidden("This server is ACL'd from that room"))))?;
	}

	services
		.server_keys
		.hash_and_sign_event(&mut signed_event, &room_version_rules)
		.map_err(|e| err!(Request(InvalidParam("Failed to sign event: {e}"))))?;

	// Add event_id back
	signed_event
		.insert("event_id".to_owned(), CanonicalJsonValue::String(body.event_id.to_string()));

	let mut invite_state = body.invite_room_state.clone();
	let pdu = PduEvent::from_id_val(&body.event_id, signed_event.clone())
		.expect("must be able to create PDU object");
	invite_state.push(RawStrippedState::Pdu(serde_json::value::to_raw_value(&signed_event)?));

	// If we are active in the room, the remote server will notify us about the
	// join/invite through /send. If we are not in the room, we need to manually
	// record the invited state for client /sync through update_membership(), and
	// send the invite PDU to the relevant appservices.
	if !resident {
		// We will start by recording the room's create event as an outlier.
		// This will allow us to recognise it later in case the sender revokes the
		// invite over federation later. We could store more state from the invite
		// request, but we will get that during send_join anyway.
		// This is safe to just add directly as an outlier as we already auth checked it
		// during validation.
		services
			.rooms
			.outlier
			.add_pdu_outlier(&create_event_id, create_event_json);

		services
			.rooms
			.state_cache
			.mark_as_invited(
				&recipient_user,
				&body.room_id,
				&sender_user,
				invite_state,
				body.via.clone(),
			)
			.await?;

		services
			.rooms
			.state_cache
			.update_joined_count(&body.room_id)
			.await;

		services.sync.wake(&recipient_user).await;

		for appservice in services.appservice.read().await.values() {
			if appservice.is_user_match(&recipient_user) {
				let transaction_id = general_purpose::URL_SAFE_NO_PAD
					.encode(sha256::hash(pdu.event_id.as_bytes()))
					.into();

				let request = ruma::api::appservice::event::push_events::v1::Request::new(
					transaction_id,
					vec![pdu.to_format()],
				);

				services
					.sending
					.send_appservice_request(appservice.registration.clone(), request)
					.await
					.map_err(|e| {
						error!(
							"failed to notify appservice {} about incoming invite: {e}",
							appservice.registration.id
						);
						err!(BadServerResponse(
							"Failed to notify appservice about incoming invite."
						))
					})?;
			}
		}
	}

	Ok(create_invite::v2::Response::new(
		services
			.sending
			.convert_to_outgoing_federation_event(signed_event)
			.await,
	))
}

/// Validates the *membership event* in the invite request, per the steps listed
/// under the invite endpoint's [spec].
///
/// Returns the validated JSON body, sender user ID, and recipient user ID.
///
/// Since this function performs a PDU format check, the create event must be
/// known ahead of time. This implies validating the invite state before the
/// invite event itself.
///
/// [spec]: https://spec.matrix.org/v1.19/server-server-api/#put_matrixfederationv2inviteroomideventid
async fn validate_invite_membership_event(
	services: &crate::State,
	body: &serde_json::value::RawValue,
	room_version_rules: &RoomVersionRules,
	origin: &ServerName,
	create_event_id: OwnedEventId,
	room_id: OwnedRoomId,
	event_id: OwnedEventId,
) -> Result<(CanonicalJsonObject, OwnedUserId, OwnedUserId)> {
	trace!(?body, "Invite membership event");
	let (pdu, target_membership, sender_user, recipient_user) = validate_any_membership_event(
		services,
		body,
		room_version_rules,
		create_event_id,
		room_id,
		event_id,
	)
	.await?;

	// Ensure the sender belongs to the remote that is sending the invite
	if sender_user.server_name() != origin {
		return Err!(Request(Forbidden("Sender belongs to a different server")));
	}

	// Ensure the target user belongs to this server
	if !services
		.globals
		.server_is_ours(recipient_user.server_name())
	{
		return Err!(Request(InvalidParam("Recipient does not belong to this homeserver")));
	}

	if target_membership != MembershipState::Invite {
		return Err!(Request(BadJson("Invalid membership (expected `invite`)")));
	}

	Ok((pdu, sender_user, recipient_user))
}

/// Validates the *invite state* of an invite request, per the steps listed
/// under the endpoint's [spec].
///
/// Returns the create event's event ID, and the partial state map.
///
/// [spec]: https://spec.matrix.org/v1.19/server-server-api/#put_matrixfederationv2inviteroomideventid
async fn validate_invite_state(
	services: &crate::State,
	invite_state: &[RawStrippedState],
	room_version_rules: &RoomVersionRules,
	room_id: OwnedRoomId,
) -> Result<(OwnedEventId, HashMap<(StateEventType, StateKey), CanonicalJsonObject>)> {
	trace!(?invite_state, "Raw invite state");
	let mut invite_state_map: HashMap<(StateEventType, StateKey), _> =
		HashMap::with_capacity(invite_state.len());
	let mut create_event_id: Option<OwnedEventId> = None;

	for (idx, invite_state_event) in invite_state.iter().cloned().enumerate() {
		trace!(%idx, ?invite_state_event, "Invite state event");
		// Stripped state hasn't been sent over federation since v1.16.
		let RawStrippedState::Pdu(raw_pdu) = invite_state_event else {
			debug!(%idx, "Invite state event is not a PDU");
			return Err!(Request(InvalidParam(
				"PDU in invite state (index {idx}) violates the room event format"
			)));
		};
		let (state_event_room_id, state_event_id, state_event_json) = services
			.rooms
			.event_handler
			.parse_incoming_pdu(&raw_pdu, Some(room_version_rules))
			.await
			.map_err(|e| {
				err!(Request(InvalidParam(debug_warn!("Invalid PDU in invite state: {e}"))))
			})?;

		if state_event_room_id != room_id {
			return Err!(Request(InvalidParam(debug_warn!(
				%state_event_room_id,
				%room_id,
				"PDU in invite state ({state_event_id}) belongs to the wrong room"
			))));
		}

		services
			.server_keys
			.verify_event(&state_event_json, room_version_rules)
			.await
			.map_err(|e| {
				err!(Request(InvalidParam("Signature verification failed on invite event: {e}")))
			})?;

		let Some(state_key) = state_event_json.get("state_key").and_then(|k| k.as_str()) else {
			return Err!(Request(InvalidParam(debug_info!(
				"PDU in invite state ({state_event_id}) is not a state event"
			))));
		};
		let Some(event_type) = state_event_json.get("type").and_then(|k| k.as_str()) else {
			return Err!(Request(InvalidParam(debug_warn!(
				"PDU in invite state ({state_event_id}) is not an event?"
			))));
		};

		let key = StateEventType::from(event_type).with_state_key(state_key);
		match invite_state_map.entry(key) {
			| Entry::Occupied(entry) =>
				return Err!(Request(InvalidParam(
					"Duplicate state events in invite state for state key: {:?}",
					entry.key(),
				))),
			| Entry::Vacant(entry) => {
				if entry.key().0 == StateEventType::RoomCreate {
					// Ensure this is a legal create event.
					let pdu_event =
						PduEvent::from_id_val(&state_event_id, state_event_json.clone())
							.expect("must be able to create pdu event from event json");
					debug!("Validating discovered create event in invite room state");
					validate_invite_create_event(&pdu_event, room_version_rules).await?;
					create_event_id = Some(state_event_id);
				}
				entry.insert(state_event_json);
			},
		}
	}
	let Some(create_event_id) = create_event_id else {
		return Err!(Request(InvalidParam(debug_warn!(
			parsed_state=?invite_state_map,
			"Invite state does not contain the m.room.create event"
		))));
	};
	invite_state_map.iter().try_for_each(|(key, event_json)| {
		service::rooms::event_handler::Service::pdu_format_check_1(
			event_json,
			room_version_rules,
			&create_event_id,
		)
		.map_err(|e| {
			err!(Request(InvalidParam(
				"PDU in invite state for {key:?} violates the room event format: {e}"
			)))
		})
	})?;

	Ok((create_event_id, invite_state_map))
}

#[derive(Deserialize)]
struct MFederate {
	#[serde(rename = "m.federate")]
	mfederate: Option<bool>,
}

/// Validates that a create event is suitable for the invite, namely:
///
/// 1. It passes auth checks (aka is valid)
/// 2. The room is federated (there's no point persisting unfederated rooms)
async fn validate_invite_create_event(
	pdu: &PduEvent,
	room_version_rules: &RoomVersionRules,
) -> Result {
	if !state_res::auth_check(
		room_version_rules,
		pdu,
		None,
		|_, _| async {
			unreachable!("No state should be fetched when processing a lone create event");
		},
		pdu,
	)
	.await
	.unwrap_or_default()
	{
		return Err!(Request(InvalidParam("m.room.create event fails auth check")));
	}

	let can_federate = pdu.get_content::<MFederate>()?.mfederate;
	if !can_federate.unwrap_or(true) {
		return Err!(Request(InvalidParam(
			"Cannot receive invites to a room with m.federate=false"
		)));
	}

	Ok(())
}
