//! Auth checks relevant to the `m.room.create` event specifically.
//!
//! See: https://spec.matrix.org/v1.16/rooms/v12/#authorization-rules

use ruma::{OwnedUserId, RoomVersionId, events::room::create::RoomCreateEventContent};
use serde::Deserialize;
use serde_json::from_str;

use crate::{Event, Pdu, RoomVersion, state_res::Error, trace};

// A raw representation of the create event content, for initial parsing.
// This allows us to extract fields without fully validating the event first.
#[derive(Deserialize)]
struct RawCreateContent {
	creator: Option<String>,
	room_version: Option<String>,
	additional_creators: Option<Vec<String>>,
}

// Check whether an `m.room.create` event is valid.
// This ensures that:
//
// 1. The event has no `prev_events`
// 2. If the version disallows it, the event has no `room_id` present.
// 3. If the room version is present and recognised, otherwise assume invalid.
// 4. If the room version supports it, `additional_creators` is populated with
//    valid user IDs.
// 5. If the room version supports it, `creator` is populated AND is a valid
//    user ID.
// 6. Otherwise, this event is valid.
//
// The fully deserialized `RoomCreateEventContent` is returned for further calls
// to other checks.
pub fn check_room_create(event: &Pdu) -> Result<RoomCreateEventContent, Error> {
	// Check 1: The event has no `prev_events`
	if !event.prev_events.is_empty() {
		return Err(Error::InvalidPdu("m.room.create event has prev_events".to_owned()));
	}

	let create_content = from_str::<RawCreateContent>(event.content().get())?;

	// Note: Here we attempt to both load the raw room version string and validate
	// it, and then cast it to the room features. If either step fails, we return
	// an unsupported error. If the room version is missing, it defaults to "1",
	// which we also do not support.
	//
	// This performs check 3, which then allows us to perform check 2.
	let room_version = if let Some(raw_room_version) = create_content.room_version {
		trace!("Parsing and interpreting room version: {}", raw_room_version);
		let room_version_id = RoomVersionId::try_from(raw_room_version.as_str())
			.map_err(|_| Error::Unsupported(raw_room_version))?;
		RoomVersion::new(&room_version_id)
			.map_err(|_| Error::Unsupported(room_version_id.as_str().to_owned()))?
	} else {
		return Err(Error::Unsupported("1".to_owned()));
	};

	// Check 2: If the version disallows it, the event has no `room_id` present.
	if room_version.room_ids_as_hashes && event.room_id.is_some() {
		return Err(Error::InvalidPdu(
			"m.room.create event has room_id but room version disallows it".to_owned(),
		));
	}

	// Check 4: If the room version supports it, `additional_creators` is populated
	// with valid user IDs.
	if room_version.explicitly_privilege_room_creators {
		if let Some(additional_creators) = create_content.additional_creators {
			for creator in additional_creators {
				trace!("Validating additional creator user ID: {}", creator);
				if OwnedUserId::parse(&creator).is_err() {
					return Err(Error::InvalidPdu(format!(
						"Invalid user ID in additional_creators: {creator}"
					)));
				}
			}
		}
	}

	// Check 5: If the room version supports it, `creator` is populated AND is a
	// valid user ID.
	if !room_version.use_room_create_sender {
		if let Some(creator) = create_content.creator {
			trace!("Validating creator user ID: {}", creator);
			if OwnedUserId::parse(&creator).is_err() {
				return Err(Error::InvalidPdu(format!("Invalid user ID in creator: {creator}")));
			}
		} else {
			return Err(Error::InvalidPdu(
				"m.room.create event missing creator field".to_owned(),
			));
		}
	}

	// Deserialise into the full create event for future checks.
	Ok(from_str::<RoomCreateEventContent>(event.content().get())?)
}
