use std::cmp;

use conduwuit_core::{
	Err, Error, Result, err, implement,
	matrix::{
		event::{Event, gen_event_id},
		pdu::{EventHash, PduBuilder, PduEvent},
		state_res::{self, RoomVersion},
	},
	utils::{self, IterStream, ReadyExt, stream::TryIgnore},
};
use futures::{StreamExt, TryStreamExt, future, future::ready};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, RoomId, RoomVersionId, UserId,
	canonical_json::to_canonical_value,
	events::{StateEventType, TimelineEventType, room::create::RoomCreateEventContent},
	uint,
};
use serde_json::value::to_raw_value;
use tracing::warn;

use super::RoomMutexGuard;

#[implement(super::Service)]
pub async fn create_hash_and_sign_event(
	&self,
	pdu_builder: PduBuilder,
	sender: &UserId,
	room_id: &RoomId,
	_mutex_lock: &RoomMutexGuard, /* Take mutex guard to make sure users get the room
	                               * state mutex */
) -> Result<(PduEvent, CanonicalJsonObject)> {
	let PduBuilder {
		event_type,
		content,
		unsigned,
		state_key,
		redacts,
		timestamp,
	} = pdu_builder;

	let prev_events: Vec<OwnedEventId> = self
		.services
		.state
		.get_forward_extremities(room_id)
		.take(20)
		.map(Into::into)
		.collect()
		.await;

	// If there was no create event yet, assume we are creating a room
	let room_version_id = self
		.services
		.state
		.get_room_version(room_id)
		.await
		.or_else(|_| {
			if event_type == TimelineEventType::RoomCreate {
				let content: RoomCreateEventContent = serde_json::from_str(content.get())?;
				Ok(content.room_version)
			} else {
				Err(Error::InconsistentRoomState(
					"non-create event for room of unknown version",
					room_id.to_owned(),
				))
			}
		})?;

	let room_version = RoomVersion::new(&room_version_id).expect("room version is supported");

	let auth_events = self
		.services
		.state
		.get_auth_events(room_id, &event_type, sender, state_key.as_deref(), &content)
		.await?;

	// Our depth is the maximum depth of prev_events + 1
	let depth = prev_events
		.iter()
		.stream()
		.map(Ok)
		.and_then(|event_id| self.get_pdu(event_id))
		.and_then(|pdu| future::ok(pdu.depth))
		.ignore_err()
		.ready_fold(uint!(0), cmp::max)
		.await
		.saturating_add(uint!(1));

	let mut unsigned = unsigned.unwrap_or_default();

	if let Some(state_key) = &state_key {
		if let Ok(prev_pdu) = self
			.services
			.state_accessor
			.room_state_get(room_id, &event_type.to_string().into(), state_key)
			.await
		{
			unsigned.insert("prev_content".to_owned(), prev_pdu.get_content_as_value());
			unsigned.insert("prev_sender".to_owned(), serde_json::to_value(prev_pdu.sender())?);
			unsigned
				.insert("replaces_state".to_owned(), serde_json::to_value(prev_pdu.event_id())?);
		}
	}

	if event_type != TimelineEventType::RoomCreate && prev_events.is_empty() {
		return Err!(Request(Unknown("Event incorrectly had zero prev_events.")));
	}
	if state_key.is_none() && depth.lt(&uint!(2)) {
		// The first two events in a room are always m.room.create and m.room.member,
		// so any other events with that same depth are illegal.
		warn!(
			"Had unsafe depth {depth} when creating non-state event in {room_id}. Cowardly \
			 aborting"
		);
		return Err!(Request(Unknown("Unsafe depth for non-state event.")));
	}

	let mut pdu = PduEvent {
		event_id: ruma::event_id!("$thiswillbefilledinlater").into(),
		room_id: room_id.to_owned(),
		sender: sender.to_owned(),
		origin: None,
		origin_server_ts: timestamp.map_or_else(
			|| {
				utils::millis_since_unix_epoch()
					.try_into()
					.expect("u64 fits into UInt")
			},
			|ts| ts.get(),
		),
		kind: event_type,
		content,
		state_key,
		prev_events,
		depth,
		auth_events: auth_events
			.values()
			.map(|pdu| pdu.event_id.clone())
			.collect(),
		redacts,
		unsigned: if unsigned.is_empty() {
			None
		} else {
			Some(to_raw_value(&unsigned)?)
		},
		hashes: EventHash { sha256: "aaa".to_owned() },
		signatures: None,
	};

	let auth_fetch = |k: &StateEventType, s: &str| {
		let key = (k.clone(), s.into());
		ready(auth_events.get(&key).map(ToOwned::to_owned))
	};

	let auth_check = state_res::auth_check(
		&room_version,
		&pdu,
		None, // TODO: third_party_invite
		auth_fetch,
	)
	.await
	.map_err(|e| err!(Request(Forbidden(warn!("Auth check failed: {e:?}")))))?;

	if !auth_check {
		return Err!(Request(Forbidden("Event is not authorized.")));
	}

	// Hash and sign
	let mut pdu_json = utils::to_canonical_object(&pdu).map_err(|e| {
		err!(Request(BadJson(warn!("Failed to convert PDU to canonical JSON: {e}"))))
	})?;

	// room v3 and above removed the "event_id" field from remote PDU format
	match room_version_id {
		| RoomVersionId::V1 | RoomVersionId::V2 => {},
		| _ => {
			pdu_json.remove("event_id");
		},
	}

	// Add origin because synapse likes that (and it's required in the spec)
	pdu_json.insert(
		"origin".to_owned(),
		to_canonical_value(self.services.globals.server_name())
			.expect("server name is a valid CanonicalJsonValue"),
	);

	if let Err(e) = self
		.services
		.server_keys
		.hash_and_sign_event(&mut pdu_json, &room_version_id)
	{
		return match e {
			| Error::Signatures(ruma::signatures::Error::PduSize) => {
				Err!(Request(TooLarge("Message/PDU is too long (exceeds 65535 bytes)")))
			},
			| _ => Err!(Request(Unknown(warn!("Signing event failed: {e}")))),
		};
	}

	// Generate event id
	pdu.event_id = gen_event_id(&pdu_json, &room_version_id)?;

	pdu_json.insert("event_id".into(), CanonicalJsonValue::String(pdu.event_id.clone().into()));

	// Check with the policy server
	match self
		.services
		.event_handler
		.ask_policy_server(&pdu, room_id)
		.await
	{
		| Ok(true) => {},
		| Ok(false) => {
			return Err!(Request(Forbidden(debug_warn!(
				"Policy server marked this event as spam"
			))));
		},
		| Err(e) => {
			// fail open
			warn!("Failed to check event with policy server: {e}");
		},
	}

	// Generate short event id
	let _shorteventid = self
		.services
		.short
		.get_or_create_shorteventid(&pdu.event_id)
		.await;

	Ok((pdu, pdu_json))
}
