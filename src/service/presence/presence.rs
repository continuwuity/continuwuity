use conduwuit::{Error, Result, utils};
use ruma::{
	UInt, UserId,
	events::presence::{PresenceEvent, PresenceEventContent},
	presence::PresenceState,
};
use serde::{Deserialize, Serialize};

use crate::users;

/// Represents data required to be kept in order to implement the presence
/// specification.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(super) struct Presence {
	state: PresenceState,
	currently_active: bool,
	last_active_ts: u64,
	status_msg: Option<String>,
}

impl Presence {
	#[must_use]
	pub(super) fn new(
		state: PresenceState,
		currently_active: bool,
		last_active_ts: u64,
		status_msg: Option<String>,
	) -> Self {
		Self {
			state,
			currently_active,
			last_active_ts,
			status_msg,
		}
	}

	pub(super) fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
		serde_json::from_slice(bytes)
			.map_err(|_| Error::bad_database("Invalid presence data in database"))
	}

	/// Creates a PresenceEvent from available data.
	pub(super) async fn to_presence_event(
		&self,
		user_id: &UserId,
		users: &users::Service,
	) -> PresenceEvent {
		let now = utils::millis_since_unix_epoch();
		let last_active_ago = Some(UInt::new_saturating(now.saturating_sub(self.last_active_ts)));
		let mut content = PresenceEventContent::new(self.state.clone());
		content.status_msg.clone_from(&self.status_msg);
		content.currently_active = Some(self.currently_active);
		content.last_active_ago = last_active_ago;
		content.displayname = users.displayname(user_id).await.ok();
		content.avatar_url = users.avatar_url(user_id).await.ok();

		PresenceEvent { sender: user_id.to_owned(), content }
	}
}
