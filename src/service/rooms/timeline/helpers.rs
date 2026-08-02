//! Helpers for submitting events with the right checks performed

use conduwuit::{
	Err, Result, err,
	matrix::pdu::{PartialPdu, sticky},
};
use ruma::{
	MilliSecondsSinceUnixEpoch, OwnedEventId, RoomId, UserId,
	events::{
		AnyStateEventContent, StateEventType,
		room::{
			canonical_alias::RoomCanonicalAliasEventContent,
			history_visibility::{HistoryVisibility, RoomHistoryVisibilityEventContent},
			join_rules::{JoinRule, RoomJoinRulesEventContent},
			member::{MembershipState, RoomMemberEventContent},
			server_acl::RoomServerAclEventContent,
		},
		sticky::StickyDurationMs,
	},
	serde::Raw,
};

use crate::rooms::state::RoomMutexGuard;

impl super::Service {
	#[allow(clippy::too_many_arguments)]
	pub async fn send_state_event_for_key(
		&self,
		sender: &UserId,
		room_id: &RoomId,
		state_lock: &RoomMutexGuard,
		event_type: &StateEventType,
		content: &Raw<AnyStateEventContent>,
		state_key: &str,
		sticky_duration_ms: Option<StickyDurationMs>,
		timestamp: Option<MilliSecondsSinceUnixEpoch>,
	) -> Result<OwnedEventId> {
		let mut content: Raw<AnyStateEventContent> = content.clone();
		self.assert_allowed_to_send_state_event(room_id, event_type, state_key, &mut content)
			.await?;

		let content = serde_json::from_str(content.json().get())
			.map_err(|e| err!(Request(BadJson("Invalid JSON body: {e}"))))?;

		let event_id = self
			.build_and_append_pdu(
				PartialPdu {
					event_type: event_type.to_string().into(),
					content,
					sticky: self
						.services
						.config
						.allow_sticky_events
						.then_some(sticky_duration_ms)
						.flatten()
						.map(sticky::object),
					state_key: Some(state_key.into()),
					timestamp,
					..Default::default()
				},
				sender,
				Some(room_id),
				state_lock,
			)
			.await?;

		Ok(event_id)
	}

	async fn assert_allowed_to_send_state_event(
		&self,
		room_id: &RoomId,
		event_type: &StateEventType,
		state_key: &str,
		json: &mut Raw<AnyStateEventContent>,
	) -> Result {
		match event_type {
			| StateEventType::RoomCreate => {
				return Err!(Request(BadJson(debug_warn!(
					%room_id,
					"You cannot update m.room.create after a room has been created."
				))));
			},
			| StateEventType::RoomServerAcl =>
				self.assert_allowed_to_send_room_server_acl_event(room_id, json)
					.await?,
			| StateEventType::RoomEncryption =>
			// Forbid m.room.encryption if encryption is disabled
				if !self.services.config.allow_encryption {
					return Err!(Request(Forbidden(
						"Encryption is disabled on this homeserver."
					)));
				},
			| StateEventType::RoomJoinRules =>
				self.assert_allowed_to_send_room_join_rules_event(room_id, json)
					.await?,
			| StateEventType::RoomHistoryVisibility =>
				self.assert_allowed_to_send_room_history_visibility_event(room_id, json)
					.await?,
			| StateEventType::RoomCanonicalAlias =>
				self.assert_allowed_to_send_room_canonical_alias_event(room_id, json)
					.await?,
			| StateEventType::RoomMember =>
				self.assert_allowed_to_send_room_member_event(room_id, state_key, json)
					.await?,
			| _ => (),
		}

		Ok(())
	}

	async fn assert_allowed_to_send_room_server_acl_event(
		&self,
		room_id: &RoomId,
		json: &Raw<AnyStateEventContent>,
	) -> Result {
		// prevents common ACL paw-guns as ACL management is difficult and prone to
		// irreversible mistakes

		let acl_content = json
			.deserialize_as_unchecked::<RoomServerAclEventContent>()
			.map_err(|e| {
				err!(Request(BadJson(debug_warn!("Room server ACL event is invalid: {e}"))))
			})?;

		let allow_has_wildcard = acl_content.allow.iter().any(|entry| entry == "*");
		let deny_has_wildcard = acl_content.deny.iter().any(|entry| entry == "*");
		let allow_has_server = acl_content
			.allow
			.iter()
			.any(|entry| entry == self.services.globals.server_name().as_str());

		if acl_content.allow.is_empty() {
			return Err!(Request(BadJson(debug_warn!(
				%room_id,
				"Sending an ACL event with an empty allow key will permanently \
				brick the room for non-conduwuit's as this equates to no servers \
				being allowed to participate in this room."
			))));
		}

		if allow_has_wildcard && deny_has_wildcard {
			return Err!(Request(BadJson(debug_warn!(
				%room_id,
				"Sending an ACL event with a deny and allow key value of \"*\" will \
				permanently brick the room for non-conduwuit's as this equates to \
				no servers being allowed to participate in this room."
			))));
		}

		if deny_has_wildcard
			&& !acl_content.is_allowed(self.services.globals.server_name())
			&& !allow_has_server
		{
			return Err!(Request(BadJson(debug_warn!(
				%room_id,
				"Sending an ACL event with a deny key value of \"*\" and without \
				your own server name in the allow key will result in you being \
				unable to participate in this room."
			))));
		}

		if !allow_has_wildcard
			&& !acl_content.is_allowed(self.services.globals.server_name())
			&& !allow_has_server
		{
			return Err!(Request(BadJson(debug_warn!(
				%room_id,
				"Sending an ACL event for an allow key without \"*\" and without \
				your own server name in the allow key will result in you being \
				unable to participate in this room."
			))));
		}
		Ok(())
	}

	async fn assert_allowed_to_send_room_join_rules_event(
		&self,
		room_id: &RoomId,
		json: &Raw<AnyStateEventContent>,
	) -> Result {
		// admin room is a sensitive room, it should not ever be made public
		if let Ok(admin_room_id) = self.services.admin.get_admin_room().await
			&& admin_room_id == room_id
		{
			let join_rule = json
				.deserialize_as_unchecked::<RoomJoinRulesEventContent>()
				.map_err(|e| {
					err!(Request(BadJson(debug_warn!("Room join rules event is invalid: {e}"))))
				})?;

			if join_rule.join_rule == JoinRule::Public {
				return Err!(Request(Forbidden(
					"Admin room is a sensitive room, it cannot be made public"
				)));
			}
		}

		Ok(())
	}

	async fn assert_allowed_to_send_room_history_visibility_event(
		&self,
		room_id: &RoomId,
		json: &Raw<AnyStateEventContent>,
	) -> Result {
		// admin room is a sensitive room, it should not ever be made world readable

		if let Ok(admin_room_id) = self.services.admin.get_admin_room().await
			&& admin_room_id == room_id
		{
			let visibility_content = json
				.deserialize_as_unchecked::<RoomHistoryVisibilityEventContent>()
				.map_err(|e| {
					err!(Request(BadJson(debug_warn!(
						"Room history visibility event is invalid: {e}"
					))))
				})?;

			if visibility_content.history_visibility == HistoryVisibility::WorldReadable {
				return Err!(Request(Forbidden(
					"Admin room is a sensitive room, it cannot be made public"
				)));
			}
		}

		Ok(())
	}

	async fn assert_allowed_to_send_room_canonical_alias_event(
		&self,
		room_id: &RoomId,
		json: &Raw<AnyStateEventContent>,
	) -> Result {
		let canonical_alias_content = json
			.deserialize_as_unchecked::<RoomCanonicalAliasEventContent>()
			.map_err(|e| {
				err!(Request(BadJson(debug_warn!("Room canonical alias event is invalid: {e}"))))
			})?;

		let mut aliases = canonical_alias_content.alt_aliases.clone();

		if let Some(alias) = canonical_alias_content.alias {
			aliases.push(alias);
		}

		for alias in aliases {
			let (alias_room_id, _) = self
				.services
				.alias
				.resolve_alias(&alias)
				.await
				.map_err(|e| err!(Request(Unknown("Failed resolving alias \"{alias}\": {e}"))))?;

			if alias_room_id != room_id {
				return Err!(Request(BadAlias(
					"Room alias {alias} does not belong to room {room_id}"
				)));
			}
		}

		Ok(())
	}

	async fn assert_allowed_to_send_room_member_event(
		&self,
		room_id: &RoomId,
		state_key: &str,
		json: &mut Raw<AnyStateEventContent>,
	) -> Result {
		let mut membership_content = json
			.deserialize_as_unchecked::<RoomMemberEventContent>()
			.map_err(|e| {
				err!(Request(BadJson(debug_warn!("Room member event is invalid: {e}"))))
			})?;

		let Ok(state_key) = UserId::parse(state_key) else {
			return Err!(Request(BadJson(
				"Membership event has invalid or non-existent state key"
			)));
		};

		let Some(authorising_user) = membership_content.join_authorized_via_users_server else {
			return Ok(());
		};

		// join_authorized_via_users_server must be thrown away, if user is
		// already a member of the room.
		if self
			.services
			.state_cache
			.is_joined(&state_key, room_id)
			.await
		{
			membership_content.join_authorized_via_users_server = None;
			*json = Raw::<AnyStateEventContent>::from_json_string(serde_json::to_string(
				&membership_content,
			)?)?;
			return Ok(());
		}

		if membership_content.membership != MembershipState::Join {
			return Err!(Request(BadJson(
				"join_authorised_via_users_server is only for member joins"
			)));
		}

		if !self.services.globals.user_is_local(&authorising_user) {
			return Err!(Request(InvalidParam(
				"Authorising user {authorising_user} does not belong to this homeserver"
			)));
		}

		if !self
			.services
			.state_cache
			.is_joined(&authorising_user, room_id)
			.await
		{
			return Err!(Request(InvalidParam(
				"Authorising user {authorising_user} is not in the room, they cannot authorise \
				 the join."
			)));
		}

		Ok(())
	}
}
