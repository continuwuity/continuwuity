use ruma::{OwnedUserId, events::room::power_levels::RoomPowerLevelsEventContent};

use crate::{
	Event, Pdu, RoomVersion,
	state_res::{Error, event_auth::context::UserPower},
};

/// Verifies that a m.room.power_levels event is well-formed according to the
/// Matrix specification.
///
/// Creators must contain the m.room.create sender and any additional creators.
pub async fn check_power_levels(
	room_version: &RoomVersion,
	event: &Pdu,
	current_power_levels: Option<&RoomPowerLevelsEventContent>,
	creators: Vec<OwnedUserId>,
) -> Result<(), Error> {
	let content = event
		.get_content::<RoomPowerLevelsEventContent>()
		.map_err(|e| {
			Error::InvalidPdu(format!("m.room.power_levels event has invalid content: {}", e))
		})?;

	// If any of the properties users_default, events_default, state_default, ban,
	// redact, kick, or invite in content are present and not an integer, reject.
	//
	// If either of the properties events or notifications in content are present
	// and not an object with values that are integers, reject.
	//
	// NOTE: Deserialisation fails if this is not the case, so we don't need to
	// check these here.

	// If the users property in content is not an object with keys that are valid
	// user IDs with values that are integers (or a string that is an integer),
	// reject.
	while let Some(user_id) = content.users.keys().next() {
		// NOTE: Deserialisation fails if the power level is not an integer, so we don't
		// need to check that here.

		if let Err(e) = user_id.validate_historical() {
			return Err(Error::InvalidPdu(format!(
				"m.room.power_levels event has invalid user ID in users map: {}",
				e
			)));
		}
		// Since v12, If the users property in content contains the sender of the
		// m.room.create event or any of the additional_creators array (if present)
		// from the content of the m.room.create event, reject.
		if room_version.explicitly_privilege_room_creators && creators.contains(user_id) {
			return Err(Error::InvalidPdu(
				"m.room.power_levels event users map contains a room creator".to_string(),
			));
		}
	}

	// If there is no previous m.room.power_levels event in the room, allow.
	if current_power_levels.is_none() {
		return Ok(());
	}
	let current_power_levels = current_power_levels.unwrap();

	// For the properties users_default, events_default, state_default, ban, redact,
	// kick, invite check if they were added, changed or removed. For each found
	// alteration:
	// If the current value is higher than the sender’s current power level, reject.
	// If the new value is higher than the sender’s current power level, reject.
	let sender = event.sender();
	let rank = if room_version.explicitly_privilege_room_creators {
		if creators.contains(&sender.to_owned()) {
			UserPower::Creator
		} else {
			UserPower::Standard
		}
	} else {
		UserPower::Standard
	};
	let sender_pl = current_power_levels
		.users
		.get(sender)
		.unwrap_or(&current_power_levels.users_default);

	if rank != UserPower::Creator {
		let checks = [
			("users_default", current_power_levels.users_default, content.users_default),
			("events_default", current_power_levels.events_default, content.events_default),
			("state_default", current_power_levels.state_default, content.state_default),
			("ban", current_power_levels.ban, content.ban),
			("redact", current_power_levels.redact, content.redact),
			("kick", current_power_levels.kick, content.kick),
			("invite", current_power_levels.invite, content.invite),
		];

		for (name, old_value, new_value) in checks.iter() {
			if old_value != new_value {
				if *old_value > *sender_pl {
					return Err(Error::AuthConditionFailed(format!(
						"sender cannot change level for {}",
						name
					)));
				}
				if *new_value > *sender_pl {
					return Err(Error::AuthConditionFailed(format!(
						"sender cannot raise level for {} to {}",
						name, new_value
					)));
				}
			}
		}

		// For each entry being changed in, or removed from, the events
		// property:
		// If the current value is greater than the sender’s current power level,
		// reject.
		for (event_type, new_value) in content.events.iter() {
			let old_value = current_power_levels.events.get(event_type);
			if old_value != Some(new_value) {
				let old_pl = old_value.unwrap_or(&current_power_levels.events_default);
				if *old_pl > *sender_pl {
					return Err(Error::AuthConditionFailed(format!(
						"sender cannot change event level for {}",
						event_type
					)));
				}
				if *new_value > *sender_pl {
					return Err(Error::AuthConditionFailed(format!(
						"sender cannot raise event level for {} to {}",
						event_type, new_value
					)));
				}
			}
		}

		// For each entry being changed in, or removed from, the events or
		// notifications properties:
		// If the current value is greater than the sender’s current power
		// level, reject.
		// If the new value is greater than the sender’s current power level,
		// reject.
		// TODO after making ruwuma's notifications value a BTreeMap

		// For each entry being added to, or changed in, the users property:
		// If the new value is greater than the sender’s current power level, reject.
		for (user_id, new_value) in content.users.iter() {
			let old_value = current_power_levels.users.get(user_id);
			if old_value != Some(new_value) {
				if *new_value > *sender_pl {
					return Err(Error::AuthConditionFailed(format!(
						"sender cannot raise user level for {} to {}",
						user_id, new_value
					)));
				}
			}
		}
	}

	Ok(())
}
