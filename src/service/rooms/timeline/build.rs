use std::{collections::HashSet, iter::once};

use conduwuit::{RoomVersion, trace};
use conduwuit_core::{
	Err, Result, implement,
	matrix::{event::Event, pdu::PduBuilder},
	utils::{IterStream, ReadyExt},
};
use futures::{FutureExt, StreamExt};
use ruma::{
	OwnedEventId, OwnedServerName, RoomId, RoomVersionId, UserId,
	events::{
		TimelineEventType,
		room::{
			create::RoomCreateEventContent,
			member::{MembershipState, RoomMemberEventContent},
			redaction::RoomRedactionEventContent,
		},
	},
};

use super::RoomMutexGuard;

/// Creates a new persisted data unit and adds it to a room. This function
/// takes a roomid_mutex_state, meaning that only this function is able to
/// mutate the room state.
#[implement(super::Service)]
#[tracing::instrument(skip(self, state_lock), level = "trace")]
pub async fn build_and_append_pdu(
	&self,
	pdu_builder: PduBuilder,
	sender: &UserId,
	room_id: Option<&RoomId>,
	state_lock: &RoomMutexGuard,
) -> Result<OwnedEventId> {
	let (pdu, pdu_json) = self
		.create_hash_and_sign_event(pdu_builder, sender, room_id, state_lock)
		.await?;

	let room_id = pdu.room_id_or_hash();
	if self.services.admin.is_admin_room(&room_id).await {
		self.check_pdu_for_admin_room(&pdu, sender).boxed().await?;
	}

	// If redaction event is not authorized, do not append it to the timeline
	if *pdu.kind() == TimelineEventType::RoomRedaction {
		use RoomVersionId::*;
		trace!("Running redaction checks for room {room_id}");
		match self.services.state.get_room_version(&room_id).await? {
			| V1 | V2 | V3 | V4 | V5 | V6 | V7 | V8 | V9 | V10 => {
				if let Some(redact_id) = pdu.redacts() {
					if !self
						.services
						.state_accessor
						.user_can_redact(redact_id, pdu.sender(), &room_id, false)
						.await?
					{
						return Err!(Request(Forbidden("User cannot redact this event.")));
					}
				}
			},
			| _ => {
				let content: RoomRedactionEventContent = pdu.get_content()?;
				if let Some(redact_id) = &content.redacts {
					if !self
						.services
						.state_accessor
						.user_can_redact(redact_id, pdu.sender(), &room_id, false)
						.await?
					{
						return Err!(Request(Forbidden("User cannot redact this event.")));
					}
				}
			},
		}
	}

	if *pdu.kind() == TimelineEventType::RoomMember {
		trace!("Running room member checks for room {room_id}");
		let content: RoomMemberEventContent = pdu.get_content()?;

		if content.join_authorized_via_users_server.is_some()
			&& content.membership != MembershipState::Join
		{
			return Err!(Request(BadJson(
				"join_authorised_via_users_server is only for member joins"
			)));
		}

		if content
			.join_authorized_via_users_server
			.as_ref()
			.is_some_and(|authorising_user| {
				!self.services.globals.user_is_local(authorising_user)
			}) {
			return Err!(Request(InvalidParam(
				"Authorising user does not belong to this homeserver"
			)));
		}
	}
	if *pdu.kind() == TimelineEventType::RoomCreate {
		trace!("Running room create checks for room {room_id}");
		let content: RoomCreateEventContent = pdu.get_content()?;
		let room_features = RoomVersion::new(&content.room_version)?;
		if room_features.room_ids_as_hashes {
			// bootstrap shortid for room
			self.services
				.short
				.get_or_create_shortroomid(&room_id)
				.await;
		}
	}

	// We append to state before appending the pdu, so we don't have a moment in
	// time with the pdu without it's state. This is okay because append_pdu can't
	// fail.
	trace!("Appending {} state for room {room_id}", pdu.event_id());
	let statehashid = self.services.state.append_to_state(&pdu, &room_id).await?;

	trace!("Generating raw ID for PDU {}", pdu.event_id());
	let pdu_id = self
		.append_pdu(
			&pdu,
			pdu_json,
			// Since this PDU references all pdu_leaves we can update the leaves
			// of the room
			once(pdu.event_id()),
			state_lock,
			&room_id,
		)
		.boxed()
		.await?;

	// We set the room state after inserting the pdu, so that we never have a moment
	// in time where events in the current room state do not exist
	trace!("Setting room state for room {room_id}");
	self.services
		.state
		.set_room_state(&room_id, statehashid, state_lock);

	let mut servers: HashSet<OwnedServerName> = self
		.services
		.state_cache
		.room_servers(&room_id)
		.map(ToOwned::to_owned)
		.collect()
		.await;

	// In case we are kicking or banning a user, we need to inform their server of
	// the change
	if *pdu.kind() == TimelineEventType::RoomMember {
		if let Some(state_key_uid) = &pdu
			.state_key
			.as_ref()
			.and_then(|state_key| UserId::parse(state_key.as_str()).ok())
		{
			servers.insert(state_key_uid.server_name().to_owned());
		}
	}

	// Remove our server from the server list since it will be added to it by
	// room_servers() and/or the if statement above
	servers.remove(self.services.globals.server_name());

	trace!("Sending PDU {} to {} servers", pdu.event_id(), servers.len());
	self.services
		.sending
		.send_pdu_servers(servers.iter().map(AsRef::as_ref).stream(), &pdu_id)
		.await?;

	trace!("Event {} in room {:?} has been appended", pdu.event_id(), room_id);
	Ok(pdu.event_id().to_owned())
}

#[implement(super::Service)]
#[tracing::instrument(skip_all, level = "debug")]
async fn check_pdu_for_admin_room<Pdu>(&self, pdu: &Pdu, sender: &UserId) -> Result
where
	Pdu: Event + Send + Sync,
{
	match pdu.kind() {
		| TimelineEventType::RoomEncryption => {
			return Err!(Request(Forbidden(error!("Encryption not supported in admins room."))));
		},
		| TimelineEventType::RoomMember => {
			let target = pdu
				.state_key()
				.filter(|v| v.starts_with('@'))
				.unwrap_or(sender.as_str());

			let server_user = &self.services.globals.server_user.to_string();

			let content: RoomMemberEventContent = pdu.get_content()?;
			match content.membership {
				| MembershipState::Leave => {
					if target == server_user {
						return Err!(Request(Forbidden(error!(
							"Server user cannot leave the admins room."
						))));
					}

					let count = self
						.services
						.state_cache
						.room_members(&pdu.room_id_or_hash())
						.ready_filter(|user| self.services.globals.user_is_local(user))
						.ready_filter(|user| *user != target)
						.boxed()
						.count()
						.await;

					if count < 2 {
						return Err!(Request(Forbidden(error!(
							"Last admin cannot leave the admins room."
						))));
					}
				},

				| MembershipState::Ban if pdu.state_key().is_some() => {
					if target == server_user {
						return Err!(Request(Forbidden(error!(
							"Server cannot be banned from admins room."
						))));
					}

					let count = self
						.services
						.state_cache
						.room_members(&pdu.room_id_or_hash())
						.ready_filter(|user| self.services.globals.user_is_local(user))
						.ready_filter(|user| *user != target)
						.boxed()
						.count()
						.await;

					if count < 2 {
						return Err!(Request(Forbidden(error!(
							"Last admin cannot be banned from admins room."
						))));
					}
				},
				| _ => {},
			}
		},
		| _ => {},
	}

	Ok(())
}
