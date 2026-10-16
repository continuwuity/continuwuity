use std::{collections::HashSet, iter::once};

use conduwuit::{Pdu, err, pdu::RawId, trace};
use conduwuit_core::{
	Err, Result,
	matrix::{event::Event, pdu::PartialPdu},
	utils::{IterStream, ReadyExt},
};
use futures::{FutureExt, StreamExt};
use ruma::{
	CanonicalJsonObject, EventId, OwnedEventId, OwnedServerName, RoomId, RoomVersionId, UserId,
	events::{
		TimelineEventType,
		room::{
			member::{MembershipState, RoomMemberEventContent},
			redaction::RoomRedactionEventContent,
		},
	},
};
use serde::Deserialize;

use super::{ExtractBody, RoomMutexGuard};

impl super::Service {
	/// Creates a new persisted data unit and adds it to a room. This function
	/// takes a roomid_mutex_state, meaning that only this function is able to
	/// mutate the room state.
	#[tracing::instrument(skip(self, state_lock, partial_pdu), level = "trace")]
	pub async fn build_and_append_pdu(
		&self,
		partial_pdu: PartialPdu,
		sender: &UserId,
		room_id: Option<&RoomId>,
		state_lock: &RoomMutexGuard,
	) -> Result<OwnedEventId> {
		let (pdu, pdu_json) = self
			.create_hash_and_sign_event(partial_pdu, sender, room_id, state_lock)
			.await?;

		let room_id = pdu.room_id_or_hash();
		if self.services.admin.is_admin_room(&room_id).await {
			self.check_pdu_for_admin_room(&pdu, sender).boxed().await?;
		}

		// If redaction event is not authorized, do not append it to the
		// timeline
		let _room_version_rules = self
			.services
			.state
			.get_room_version(&room_id)
			.await?
			.rules()
			.expect("room version must have algorithm rules");
		if *pdu.kind() == TimelineEventType::RoomRedaction {
			use ruma::RoomVersionId::*;
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
			trace!("Creating shortroomid for {room_id}");
			self.services
				.short
				.get_or_create_shortroomid(&room_id)
				.await;
		}

		let prev_events: HashSet<&EventId> = pdu.prev_events().collect();
		let unreferenced: Vec<OwnedEventId> = self
			.services
			.state
			.get_forward_extremities(&room_id)
			.ready_filter(|extremity| !prev_events.contains(&**extremity))
			.collect()
			.await;

		// We set the event's state before we actually append the PDU so that
		// there isn't a moment in time where the PDU exists without its
		// associated state.
		let pdu_id = if pdu.prev_events.len() <= 1 {
			// We can simply append the event to the current state in this case,
			// and skip state resolution.
			let statehashid = self.services.state.append_to_state(&pdu, &room_id).await?;
			self.services
				.state
				.set_room_state(&room_id, statehashid, state_lock);
			self.append_pdu(
				&pdu,
				pdu_json,
				unreferenced
					.iter()
					.map(AsRef::as_ref)
					.chain(once(pdu.event_id())),
				state_lock,
				&room_id,
			)
			.boxed()
			.await
			.expect("append_pdu should be infallible")
		} else {
			// Otherwise, we have to resolve the state across the prev events
			// and go through append_incoming_pdu. Despite checking auth events,
			// we now have to do PDU check 5 to make sure we even
			// want to keep this event based on the state before it.
			self.resolve_and_append(&room_id, &pdu, pdu_json, state_lock)
				.await?
		};

		// Process admin commands for locally sent events
		if *pdu.kind() == TimelineEventType::RoomMessage {
			let content: ExtractBody = pdu.get_content()?;
			if let Some(body) = content.body {
				if let Some(source) = self
					.services
					.admin
					.is_admin_command(&pdu, &body, true)
					.await
				{
					self.services.admin.command_with_sender(
						body,
						Some(pdu.event_id().into()),
						source,
						pdu.sender.clone(),
					)?;
				}
			}
		}

		// We set the room state after inserting the pdu, so that we never have
		// a moment in time where events in the current room state do not
		// exist
		trace!("Setting room state for room {room_id}");

		self.services.sync.wake_all_joined(&room_id).await;

		if *pdu.kind() == TimelineEventType::RoomMember {
			if let Some(target_user) = pdu
				.state_key
				.as_ref()
				.and_then(|state_key| UserId::parse(state_key.as_str()).ok())
			{
				if self.services.globals.user_is_local(&target_user) {
					self.services.sync.wake(&target_user).await;
				}
			}
		}

		let mut servers: HashSet<OwnedServerName> = self
			.services
			.state_cache
			.room_servers(&room_id)
			.collect()
			.await;

		// In case we are kicking or banning a user, we need to inform their
		// server of the change
		if *pdu.kind() == TimelineEventType::RoomMember {
			if let Some(state_key_uid) = &pdu
				.state_key
				.as_ref()
				.and_then(|state_key| UserId::parse(state_key.as_str()).ok())
			{
				servers.insert(state_key_uid.server_name().to_owned());
			}
		}

		// Remove our server from the server list since it will be added to it
		// by room_servers() and/or the if statement above
		servers.remove(self.services.globals.server_name());

		trace!("Sending PDU {} to {} servers", pdu.event_id(), servers.len());
		self.services
			.sending
			.send_pdu_servers(servers.stream(), &pdu_id)
			.await?;

		trace!("Event {} in room {:?} has been appended", pdu.event_id(), room_id);
		Ok(pdu.event_id().to_owned())
	}

	/// Assert invariants about the admin room, to prevent (for example) all
	/// admins from leaving or being banned from the room
	#[tracing::instrument(skip_all, level = "debug")]
	async fn check_pdu_for_admin_room<Pdu>(&self, pdu: &Pdu, sender: &UserId) -> Result
	where
		Pdu: Event + Send + Sync,
	{
		match pdu.kind() {
			| TimelineEventType::RoomEncryption => {
				return Err!(Request(Forbidden(error!(
					"Encryption not supported in admins room."
				))));
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

	/// Resolves the state before a new event and runs it through
	/// append_incoming_pdu to persist the state update, instead of simply
	/// appending it to the current state.
	async fn resolve_and_append(
		&self,
		room_id: &RoomId,
		pdu: &Pdu,
		pdu_json: CanonicalJsonObject,
		state_lock: &RoomMutexGuard,
	) -> Result<RawId> {
		let create_event =
			if pdu.kind == TimelineEventType::RoomCreate && pdu.state_key() == Some("") {
				pdu
			} else {
				&self
					.services
					.state_accessor
					.get_room_create_event(room_id)
					.await
			};
		let room_version_rules = create_event
			.get_content::<ExtractRoomVersion>()?
			.room_version
			.rules()
			.ok_or_else(|| err!("Unsupported room version"))?;
		let (ok, state_before) = self
			.services
			.event_handler
			.state_before_check_5(
				pdu,
				&room_version_rules,
				create_event,
				self.services.globals.server_name(),
			)
			.await?;
		if !ok {
			return Err!(Request(Forbidden("Event is not authorised")));
		}

		self.append_incoming_pdu(
			pdu,
			pdu_json,
			&room_version_rules,
			state_before,
			false,
			state_lock,
		)
		.boxed()
		.await
		.transpose()
		.expect("Only soft-failed events cannot return a PduId, which is impossible in this path")
	}
}

#[derive(Deserialize)]
struct ExtractRoomVersion {
	room_version: RoomVersionId,
}
