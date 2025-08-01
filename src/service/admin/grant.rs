use std::collections::BTreeMap;

use conduwuit::{Err, Result, debug_info, debug_warn, error, implement, matrix::pdu::PduBuilder};
use ruma::{
	RoomId, UserId,
	events::{
		RoomAccountDataEventType, StateEventType,
		room::{
			member::{MembershipState, RoomMemberEventContent},
			message::RoomMessageEventContent,
			power_levels::RoomPowerLevelsEventContent,
		},
		tag::{TagEvent, TagEventContent, TagInfo},
	},
};

/// Invite the user to the conduwuit admin room.
///
/// This is equivalent to granting server admin privileges.
#[implement(super::Service)]
pub async fn make_user_admin(&self, user_id: &UserId) -> Result {
	let Ok(room_id) = self.get_admin_room().await else {
		debug_warn!(
			"make_user_admin was called without an admin room being available or created"
		);
		return Ok(());
	};

	let state_lock = self.services.state.mutex.lock(&room_id).await;

	if self.services.state_cache.is_joined(user_id, &room_id).await {
		return Err!(debug_warn!("User is already joined in the admin room"));
	}
	if self
		.services
		.state_cache
		.is_invited(user_id, &room_id)
		.await
	{
		return Err!(debug_warn!("User is already pending an invitation to the admin room"));
	}

	// Use the server user to grant the new admin's power level
	let server_user = self.services.globals.server_user.as_ref();

	// if this is our local user, just forcefully join them in the room. otherwise,
	// invite the remote user.
	if self.services.globals.user_is_local(user_id) {
		debug_info!("Inviting local user {user_id} to admin room {room_id}");
		self.services
			.timeline
			.build_and_append_pdu(
				PduBuilder::state(
					String::from(user_id),
					&RoomMemberEventContent::new(MembershipState::Invite),
				),
				server_user,
				&room_id,
				&state_lock,
			)
			.await?;

		debug_info!("Force joining local user {user_id} to admin room {room_id}");
		self.services
			.timeline
			.build_and_append_pdu(
				PduBuilder::state(
					String::from(user_id),
					&RoomMemberEventContent::new(MembershipState::Join),
				),
				user_id,
				&room_id,
				&state_lock,
			)
			.await?;
	} else {
		debug_info!("Inviting remote user {user_id} to admin room {room_id}");
		self.services
			.timeline
			.build_and_append_pdu(
				PduBuilder::state(
					user_id.to_string(),
					&RoomMemberEventContent::new(MembershipState::Invite),
				),
				server_user,
				&room_id,
				&state_lock,
			)
			.await?;
	}

	// Set power levels
	let mut room_power_levels = self
		.services
		.state_accessor
		.room_state_get_content::<RoomPowerLevelsEventContent>(
			&room_id,
			&StateEventType::RoomPowerLevels,
			"",
		)
		.await
		.unwrap_or_default();

	room_power_levels
		.users
		.insert(server_user.into(), 69420.into());
	room_power_levels.users.insert(user_id.into(), 100.into());

	self.services
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(String::new(), &room_power_levels),
			server_user,
			&room_id,
			&state_lock,
		)
		.await?;

	// Set room tag
	let room_tag = self.services.server.config.admin_room_tag.as_str();
	if !room_tag.is_empty() {
		if let Err(e) = self.set_room_tag(&room_id, user_id, room_tag).await {
			error!(?room_id, ?user_id, ?room_tag, "Failed to set tag for admin grant: {e}");
		}
	}

	if self.services.server.config.admin_room_notices {
		let welcome_message = String::from(
			"## Thank you for trying out Continuwuity!\n\nContinuwuity is a hard fork of conduwuit, which is also a hard fork of Conduit, currently in Beta. The Beta status initially was inherited from Conduit, however overtime this Beta status is rapidly becoming less and less relevant as our codebase significantly diverges more and more. Continuwuity is quite stable and very usable as a daily driver and for a low-medium sized homeserver. There is still a lot of more work to be done, but it is in a far better place than the project was in early 2024.\n\nHelpful links:\n> Source code: https://forgejo.ellis.link/continuwuation/continuwuity\n> Documentation: https://continuwuity.org/\n> Report issues: https://forgejo.ellis.link/continuwuation/continuwuity/issues\n\nFor a list of available commands, send the following message in this room: `!admin --help`\n\nHere are some rooms you can join (by typing the command into your client) -\n\nContinuwuity space: `/join #space:continuwuity.org`\nContinuwuity main room (Ask questions and get notified on updates): `/join #continuwuity:continuwuity.org`\nContinuwuity offtopic room: `/join #offtopic:continuwuity.org`",
		);

		// Send welcome message
		self.services
			.timeline
			.build_and_append_pdu(
				PduBuilder::timeline(&RoomMessageEventContent::text_markdown(welcome_message)),
				server_user,
				&room_id,
				&state_lock,
			)
			.await?;
	}

	Ok(())
}

#[implement(super::Service)]
async fn set_room_tag(&self, room_id: &RoomId, user_id: &UserId, tag: &str) -> Result {
	let mut event = self
		.services
		.account_data
		.get_room(room_id, user_id, RoomAccountDataEventType::Tag)
		.await
		.unwrap_or_else(|_| TagEvent {
			content: TagEventContent { tags: BTreeMap::new() },
		});

	event
		.content
		.tags
		.insert(tag.to_owned().into(), TagInfo::new());

	self.services
		.account_data
		.update(
			Some(room_id),
			user_id,
			RoomAccountDataEventType::Tag,
			&serde_json::to_value(event)?,
		)
		.await
}

/// Demote an admin, removing its rights.
#[implement(super::Service)]
pub async fn revoke_admin(&self, user_id: &UserId) -> Result {
	use MembershipState::{Invite, Join, Knock, Leave};

	let Ok(room_id) = self.get_admin_room().await else {
		return Err!(error!("No admin room available or created."));
	};

	let state_lock = self.services.state.mutex.lock(&room_id).await;

	let event = match self
		.services
		.state_accessor
		.get_member(&room_id, user_id)
		.await
	{
		| Err(e) if e.is_not_found() => return Err!("{user_id} was never an admin."),

		| Err(e) => return Err!(error!(?e, "Failure occurred while attempting revoke.")),

		| Ok(event) if !matches!(event.membership, Invite | Knock | Join) => {
			return Err!("Cannot revoke {user_id} in membership state {:?}.", event.membership);
		},

		| Ok(event) => {
			assert!(
				matches!(event.membership, Invite | Knock | Join),
				"Incorrect membership state to remove user."
			);

			event
		},
	};

	self.services
		.timeline
		.build_and_append_pdu(
			PduBuilder::state(user_id.to_string(), &RoomMemberEventContent {
				membership: Leave,
				reason: Some("Admin Revoked".into()),
				is_direct: None,
				join_authorized_via_users_server: None,
				third_party_invite: None,
				..event
			}),
			self.services.globals.server_user.as_ref(),
			&room_id,
			&state_lock,
		)
		.await
		.map(|_| ())
}
