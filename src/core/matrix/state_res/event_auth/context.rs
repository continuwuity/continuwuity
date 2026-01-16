//! Context for event authorisation checks

use ruma::{
	Int, OwnedUserId, UserId,
	events::{
		StateEventType,
		room::{create::RoomCreateEventContent, power_levels::RoomPowerLevelsEventContent},
	},
};

use crate::{Event, EventTypeExt, Pdu, RoomVersion, matrix::StateKey, state_res::Error};

pub enum UserPower {
	/// Creator indicates this user should be granted a power level above all.
	Creator,
	/// Standard indicates power levels should be used to determine rank.
	Standard,
}

impl PartialEq for UserPower {
	fn eq(&self, other: &Self) -> bool {
		match (self, other) {
			| (UserPower::Creator, UserPower::Creator) => true,
			| (UserPower::Standard, UserPower::Standard) => true,
			| _ => false,
		}
	}
}

/// Get the creators of the room.
/// If this room only supports one creator, a vec of one will be returned.
/// If multiple creators are supported, all will be returned, with the
/// m.room.create sender first.
pub async fn calculate_creators<FS>(
	room_version: &RoomVersion,
	fetch_state: FS,
) -> Result<Vec<OwnedUserId>, Error>
where
	FS: AsyncFn((StateEventType, StateKey)) -> Result<Option<Pdu>, Error>,
{
	let create_event = fetch_state(StateEventType::RoomCreate.with_state_key(""))
		.await?
		.ok_or_else(|| Error::InvalidPdu("Room create event not found".to_owned()))?;
	let content = create_event
		.get_content::<RoomCreateEventContent>()
		.map_err(|e| {
			Error::InvalidPdu(format!("Room create event has invalid content: {}", e))
		})?;

	if room_version.explicitly_privilege_room_creators {
		let mut creators = vec![create_event.sender().to_owned()];
		if let Some(additional) = content.additional_creators {
			for user_id in additional {
				if !creators.contains(&user_id) {
					creators.push(user_id);
				}
			}
		}
		Ok(creators)
	} else if room_version.use_room_create_sender {
		Ok(vec![create_event.sender().to_owned()])
	} else {
		// Have to check the event content
		if let Some(creator) = content.creator {
			Ok(vec![creator])
		} else {
			Err(Error::InvalidPdu("Room create event missing creator field".to_owned()))
		}
	}
}

/// Rank fetches the creatorship and power level of the target user
///
/// Returns (UserPower, power_level, Option<RoomPowerLevelsEventContent>)
/// If UserPower::Creator is returned, the power_level and
/// RoomPowerLevelsEventContent will be meaningless and can be ignored.
pub async fn get_rank<FS>(
	room_version: &RoomVersion,
	fetch_state: &FS,
	user_id: &UserId,
) -> Result<(UserPower, Int, Option<RoomPowerLevelsEventContent>), Error>
where
	FS: AsyncFn((StateEventType, StateKey)) -> Result<Option<Pdu>, Error>,
{
	let creators = calculate_creators(room_version, &fetch_state).await?;
	if creators.contains(&user_id.to_owned()) && room_version.explicitly_privilege_room_creators {
		return Ok((UserPower::Creator, Int::MAX, None));
	}

	let power_levels = fetch_state(StateEventType::RoomPowerLevels.with_state_key("")).await?;
	if let Some(power_levels) = power_levels {
		let power_levels = power_levels
			.get_content::<RoomPowerLevelsEventContent>()
			.map_err(|e| {
				Error::InvalidPdu(format!("m.room.power_levels event has invalid content: {}", e))
			})?;
		Ok((
			UserPower::Standard,
			*power_levels
				.users
				.get(user_id)
				.unwrap_or(&power_levels.users_default),
			Some(power_levels),
		))
	} else {
		// No power levels event, use defaults
		if creators[0] == user_id {
			return Ok((UserPower::Creator, Int::MAX, None));
		}
		Ok((UserPower::Standard, Int::from(0), None))
	}
}
