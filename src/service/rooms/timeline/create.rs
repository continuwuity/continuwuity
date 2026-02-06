use std::{cmp, collections::HashMap};

use conduwuit::{smallstr::SmallString, trace};
use conduwuit_core::{
	Err, Error, Result, err, implement,
	matrix::{
		event::{Event, gen_event_id},
		pdu::{EventHash, PduBuilder, PduEvent},
		state_res::{self, RoomVersion},
	},
	utils::{self, IterStream, ReadyExt, stream::TryIgnore},
	warn,
};
use futures::{StreamExt, TryStreamExt, future, future::ready};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, OwnedRoomId, RoomId, RoomVersionId,
	UserId,
	events::{StateEventType, TimelineEventType, room::create::RoomCreateEventContent},
	uint,
};
use serde_json::value::{RawValue, to_raw_value};

use super::RoomMutexGuard;

pub fn pdu_fits(owned_obj: &mut CanonicalJsonObject) -> bool {
	// room IDs, event IDs, senders, types, and state keys must all be <= 255 bytes
	if let Some(CanonicalJsonValue::String(room_id)) = owned_obj.get("room_id") {
		if room_id.len() > 255 {
			return false;
		}
	}
	if let Some(CanonicalJsonValue::String(event_id)) = owned_obj.get("event_id") {
		if event_id.len() > 255 {
			return false;
		}
	}
	if let Some(CanonicalJsonValue::String(sender)) = owned_obj.get("sender") {
		if sender.len() > 255 {
			return false;
		}
	}
	if let Some(CanonicalJsonValue::String(kind)) = owned_obj.get("type") {
		if kind.len() > 255 {
			return false;
		}
	}
	if let Some(CanonicalJsonValue::String(state_key)) = owned_obj.get("state_key") {
		if state_key.len() > 255 {
			return false;
		}
	}
	// Now check the full PDU size
	match serde_json::to_string(owned_obj) {
		| Ok(s) => s.len() <= 65535,
		| Err(_) => false,
	}
}

#[implement(super::Service)]
pub async fn create_hash_and_sign_event(
	&self,
	pdu_builder: PduBuilder,
	sender: &UserId,
	room_id: Option<&RoomId>,
	_mutex_lock: &RoomMutexGuard, /* Take mutex guard to make sure users get the room
	                               * state mutex */
) -> Result<(PduEvent, CanonicalJsonObject)> {
	#[allow(clippy::boxed_local)]
	fn from_evt(
		room_id: OwnedRoomId,
		event_type: &TimelineEventType,
		content: &RawValue,
	) -> Result<RoomVersionId> {
		if event_type == &TimelineEventType::RoomCreate {
			let content: RoomCreateEventContent = serde_json::from_str(content.get())?;
			Ok(content.room_version)
		} else {
			Err!(InconsistentRoomState("non-create event for room of unknown version", room_id))
		}
	}
	let PduBuilder {
		event_type,
		content,
		unsigned,
		state_key,
		redacts,
		timestamp,
	} = pdu_builder;
	// If there was no create event yet, assume we are creating a room
	trace!(
		"Creating event of type {} in room {}",
		event_type,
		room_id.as_ref().map_or("None", |id| id.as_str())
	);
	let room_version_id = match room_id {
		| Some(room_id) => {
			trace!(%room_id, "Looking up existing room ID");
			self.services
				.state
				.get_room_version(room_id)
				.await
				.or_else(|_| {
					from_evt(room_id.to_owned(), &event_type.clone(), &content.clone())
				})?
		},
		| None => {
			trace!("No room ID, assuming room creation");
			from_evt(
				RoomId::new(self.services.globals.server_name()),
				&event_type.clone(),
				&content.clone(),
			)?
		},
	};

	let room_version = RoomVersion::new(&room_version_id).expect("room version is supported");

	let prev_events: Vec<OwnedEventId> = match room_id {
		| Some(room_id) =>
			self.services
				.state
				.get_forward_extremities(room_id)
				.take(20)
				.map(Into::into)
				.collect()
				.await,
		| None => Vec::new(),
	};

	let auth_events: HashMap<(StateEventType, SmallString<[u8; 48]>), PduEvent> = match room_id {
		| Some(room_id) =>
			self.services
				.state
				.get_auth_events(
					room_id,
					&event_type,
					sender,
					state_key.as_deref(),
					&content,
					&room_version,
				)
				.await?,
		| None => HashMap::new(),
	};
	// Our depth is the maximum depth of prev_events + 1
	let depth = match room_id {
		| Some(_) => prev_events
			.iter()
			.stream()
			.map(Ok)
			.and_then(|event_id| self.get_pdu(event_id))
			.and_then(|pdu| future::ok(pdu.depth))
			.ignore_err()
			.ready_fold(uint!(0), cmp::max)
			.await
			.saturating_add(uint!(1)),
		| None => uint!(1),
	};

	let mut unsigned = unsigned.unwrap_or_default();

	if let Some(room_id) = room_id {
		if let Some(state_key) = &state_key {
			if let Ok(prev_pdu) = self
				.services
				.state_accessor
				.room_state_get(room_id, &event_type.clone().to_string().into(), state_key)
				.await
			{
				unsigned.insert("prev_content".to_owned(), prev_pdu.get_content_as_value());
				unsigned
					.insert("prev_sender".to_owned(), serde_json::to_value(prev_pdu.sender())?);
				unsigned.insert(
					"replaces_state".to_owned(),
					serde_json::to_value(prev_pdu.event_id())?,
				);
			}
		}
	}

	let mut pdu = PduEvent {
		event_id: ruma::event_id!("$thiswillbefilledinlater").into(),
		room_id: room_id.map(ToOwned::to_owned),
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
		hashes: EventHash { sha256: String::new() },
		signatures: None,
	};

	let auth_fetch = |k: &StateEventType, s: &str| {
		let key = (k.clone(), s.into());
		ready(auth_events.get(&key).map(ToOwned::to_owned))
	};

	let room_id_or_hash = pdu.room_id_or_hash();
	let create_pdu = match &pdu.kind {
		| TimelineEventType::RoomCreate => None,
		| _ => Some(
			self.services
				.state_accessor
				.room_state_get(&room_id_or_hash, &StateEventType::RoomCreate, "")
				.await
				.map_err(|e| {
					err!(Request(Forbidden(warn!("Failed to fetch room create event: {e}"))))
				})?,
		),
	};
	let create_event = match &pdu.kind {
		| TimelineEventType::RoomCreate => &pdu,
		| _ => create_pdu.as_ref().unwrap().as_pdu(),
	};

	let auth_check = state_res::auth_check(
		&room_version,
		&pdu,
		None, // TODO: third_party_invite
		auth_fetch,
		create_event,
	)
	.await
	.map_err(|e| err!(Request(Forbidden(warn!("Auth check failed: {e:?}")))))?;

	if !auth_check {
		return Err!(Request(Forbidden("Event is not authorized.")));
	}
	trace!(
		"Event {} in room {} is authorized",
		pdu.event_id,
		pdu.room_id.as_ref().map_or("None", |id| id.as_str())
	);

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

	trace!("hashing and signing event {}", pdu.event_id);
	if let Err(e) = self
		.services
		.server_keys
		.hash_and_sign_event(&mut pdu_json, &room_version_id)
	{
		return match e {
			| Error::Signatures { source, .. }
				if matches!(source, ruma::signatures::Error::PduSize) =>
			{
				Err!(Request(TooLarge("Message/PDU is too long (exceeds 65535 bytes)")))
			},
			| _ => Err!(Request(Unknown(warn!("Signing event failed: {e}")))),
		};
	}
	// Generate event id
	pdu.event_id = gen_event_id(&pdu_json, &room_version_id)?;
	pdu_json.insert("event_id".into(), CanonicalJsonValue::String(pdu.event_id.clone().into()));
	// Verify that the *full* PDU isn't over 64KiB.
	// Ruma only validates that it's under 64KiB before signing and hashing.
	// Has to be cloned to prevent mutating pdu_json itself :(
	if !pdu_fits(&mut pdu_json.clone()) {
		// feckin huge PDU mate
		return Err!(Request(TooLarge("Message/PDU is too long (exceeds 65535 bytes)")));
	}

	// Check with the policy server
	if room_id.is_some() {
		trace!(
			"Checking event in room {} with policy server",
			pdu.room_id.as_ref().map_or("None", |id| id.as_str())
		);
		match self
			.services
			.event_handler
			.ask_policy_server(&pdu, &mut pdu_json, pdu.room_id().expect("has room ID"), false)
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
	}

	// Generate short event id
	trace!(
		"Generating short event ID for {} in room {}",
		pdu.event_id,
		pdu.room_id.as_ref().map_or("None", |id| id.as_str())
	);
	let _shorteventid = self
		.services
		.short
		.get_or_create_shorteventid(&pdu.event_id)
		.await;

	trace!("New PDU created: {pdu:?}");
	Ok((pdu, pdu_json))
}
