use std::{
	collections::{BTreeMap, HashSet},
	sync::Arc,
};

use conduwuit::{
	debug_warn,
	pdu::{Count, ShortRoomId},
	trace,
	utils::{IterStream, TryFutureExtExt, stream::BroadbandExt},
	warn,
};
use conduwuit_core::{
	Result, err, error,
	matrix::{
		event::Event,
		pdu::{PduCount, PduEvent, PduId, RawPduId},
	},
	utils::{self, ReadyExt},
};
use futures::{StreamExt, TryFutureExt};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, RoomVersionId, UserId,
	events::{
		GlobalAccountDataEventType, TimelineEventType,
		push_rules::PushRulesEvent,
		room::{encrypted::Relation, redaction::RoomRedactionEventContent},
	},
	push::{Action, Ruleset, Tweak},
};

use super::{ExtractBody, ExtractRelatesTo, ExtractRelatesToEventId, RoomMutexGuard};
use crate::{appservice::RegistrationInfo, rooms::state_compressor::CompressedState};

impl super::Service {
	/// Append the incoming event setting the state snapshot to the state from
	/// the server that sent the event.
	#[allow(clippy::too_many_arguments)]
	pub async fn append_incoming_pdu<'a, Leaves>(
		&'a self,
		pdu: &'a PduEvent,
		pdu_json: CanonicalJsonObject,
		new_room_leaves: Leaves,
		state_ids_compressed: Arc<CompressedState>,
		soft_fail: bool,
		state_lock: &'a RoomMutexGuard,
		room_id: &'a ruma::RoomId,
	) -> Result<Option<RawPduId>>
	where
		Leaves: Iterator<Item = &'a EventId> + Send + 'a,
	{
		// We append to state before appending the pdu, so we don't have a moment in
		// time with the pdu without it's state. This is okay because append_pdu can't
		// fail.
		self.services
			.state
			.set_event_state(&pdu.event_id, room_id, state_ids_compressed)
			.await?;

		if soft_fail {
			// Nothing else to do with a soft-failed event.
			return Ok(None);
		}

		let pdu_id = self
			.append_pdu(pdu, pdu_json, new_room_leaves, state_lock, room_id)
			.await?;

		// Process admin commands for federation events
		if *pdu.kind() == TimelineEventType::RoomMessage {
			let content: ExtractBody = pdu.get_content()?;
			if let Some(body) = content.body {
				if let Some(source) = self
					.services
					.admin
					.is_admin_command(pdu, &body, false)
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

		Ok(Some(pdu_id))
	}

	/// Populates the unsigned data of a PDU
	async fn populate_unsigned(&self, pdu: &PduEvent, pdu_json: &mut CanonicalJsonObject) {
		let Some(state_key) = pdu.state_key() else {
			return; // Non-state events can't have replaced state
		};

		let CanonicalJsonValue::Object(unsigned) = pdu_json
			.entry("unsigned".into())
			.or_insert_with(|| CanonicalJsonValue::Object(BTreeMap::new()))
		else {
			return; // This shouldn't be reachable, really.
		};

		let Ok(shortstatehash) = self
			.services
			.state_accessor
			.pdu_shortstatehash(pdu.event_id())
			.await
		else {
			return;
		};
		let Ok(prev_state) = self
			.services
			.state_accessor
			.state_get(shortstatehash, &pdu.kind().to_string().into(), state_key)
			.await
		else {
			return;
		};
		unsigned.insert(
			"prev_content".to_owned(),
			CanonicalJsonValue::Object(
				utils::to_canonical_object(prev_state.get_content_as_value())
					.expect("Failed to convert prev_content into canonical JSON object"),
			),
		);
		unsigned.insert(
			String::from("prev_sender"),
			CanonicalJsonValue::String(prev_state.sender().to_string()),
		);
		unsigned.insert(
			String::from("replaces_state"),
			CanonicalJsonValue::String(prev_state.event_id().to_string()),
		);
	}

	/// Creates a new persisted data unit and adds it to a room.
	///
	/// By this point the incoming event should be fully authenticated, no auth
	/// happens in `append_pdu`.
	///
	/// Returns pdu id
	pub async fn append_pdu<'a, Leaves>(
		&'a self,
		pdu: &'a PduEvent,
		mut pdu_json: CanonicalJsonObject,
		leaves: Leaves,
		state_lock: &'a RoomMutexGuard,
		room_id: &'a ruma::RoomId,
	) -> Result<RawPduId>
	where
		Leaves: Iterator<Item = &'a EventId> + Send + 'a,
	{
		// Coalesce database writes for the remainder of this scope.
		let _cork = self.db.db.cork_and_flush();

		let shortroomid = self
			.services
			.short
			.get_shortroomid(room_id)
			.await
			.map_err(|_| err!(Database("Room does not exist")))?;

		// Make unsigned fields correct. This is not properly documented in the spec,
		// but state events need to have previous content in the unsigned field, so
		// clients can easily interpret things like membership changes

		// TODO: This needs to be refactored to add this information on the fly.
		// Because the prev content becomes part of the unsigned object in the PDU, we
		// unintentionally leak redacted or hidden content to local users.
		// See: https://forgejo.ellis.link/continuwuation/continuwuity/issues/1103
		self.populate_unsigned(pdu, &mut pdu_json).await;

		// We must keep track of all events that have been referenced.
		self.services
			.pdu_metadata
			.mark_as_referenced(room_id, pdu.prev_events().map(AsRef::as_ref));

		trace!("setting forward extremities");
		self.services
			.state
			.set_forward_extremities(room_id, leaves, state_lock)
			.await;

		let insert_lock = self.mutex_insert.lock(room_id).await;

		let count1 = self.services.globals.next_count().unwrap();

		// Mark as read first so the sending client doesn't get a notification even if
		// appending fails
		// TODO: Is this necessary? appending doesn't seem that fallible, and if it is,
		// there's bigger issues than ghost notifications.
		self.services
			.read_receipt
			.private_read_set(room_id, pdu.sender(), count1);

		self.services
			.user
			.reset_notification_counts(pdu.sender(), room_id);

		let count2 = PduCount::Normal(self.services.globals.next_count().unwrap());
		let pdu_id: RawPduId = PduId { shortroomid, shorteventid: count2 }.into();

		// Insert pdu
		self.db.append_pdu(&pdu_id, pdu, &pdu_json, count2).await;

		drop(insert_lock);

		// See if the event matches any known pushers via power level
		if *pdu.kind() != TimelineEventType::RoomCreate {
			tokio::join!(
				self.notify_local_users(pdu, &pdu_id),
				self.handle_pdu_effects(pdu, &pdu_id, shortroomid)
					.inspect_err(|e| {
						error!(
							"failed to handle PDU effects of incoming PDU {}: {e:?}",
							pdu.event_id()
						);
					})
					.ok(),
				self.aggregate_relations(pdu, count2),
			);
		}

		self.send_to_interested_appservices(pdu, &pdu_id).await;

		Ok(pdu_id)
	}

	/// Notifies local users of the incoming event with a power levels context.
	async fn notify_local_users(&self, pdu: &PduEvent, pdu_id: &RawPduId) {
		let power_levels = self
			.services
			.state_accessor
			.get_room_power_levels(pdu.room_id().unwrap())
			.await;
		let mut push_targets: HashSet<_> = self
			.services
			.state_cache
			.active_local_users_in_room(pdu.room_id().unwrap())
			// Don't notify the sender of their own events, and don't send from ignored users
			.ready_filter(|user| *user != pdu.sender())
			.filter_map(|recipient_user| async move {
				(!self.services.users.user_is_ignored(pdu.sender(), &recipient_user).await).then_some(recipient_user)
			})
			.collect()
			.await;

		let mut notifies = Vec::with_capacity(push_targets.len().saturating_add(1));
		let mut highlights = Vec::with_capacity(push_targets.len().saturating_add(1));

		if *pdu.kind() == TimelineEventType::RoomMember {
			if let Some(state_key) = pdu.state_key() {
				match UserId::parse(state_key) {
					| Ok(target_user_id) => {
						if self.services.users.is_active_local(&target_user_id).await {
							push_targets.insert(target_user_id.clone());
						}
					},
					| Err(e) => debug_warn!(user_id=?state_key, ?e, "failed to parse user ID"),
				}
			}
		}

		if push_targets.is_empty() {
			return;
		}

		let serialized = pdu.to_format();
		for user in &push_targets {
			let rules_for_user = self
				.services
				.account_data
				.get_global(user, GlobalAccountDataEventType::PushRules)
				.await
				.map_or_else(
					|_| Ruleset::server_default(user),
					|ev: PushRulesEvent| ev.content.global,
				);

			let mut highlight = false;
			let mut notify = false;

			for action in self
				.services
				.pusher
				.get_actions(
					user,
					&rules_for_user,
					power_levels.clone(),
					&serialized,
					pdu.room_id().unwrap(),
				)
				.await
			{
				match action {
					| Action::Notify => notify = true,
					| Action::SetTweak(Tweak::Highlight(
						ruma::push::HighlightTweakValue::Yes,
					)) => {
						highlight = true;
					},
					| _ => {},
				}

				// Break early if both conditions are true
				if notify && highlight {
					break;
				}
			}

			if notify {
				notifies.push(user.clone());
			}

			if highlight {
				highlights.push(user.clone());
			}

			self.services
				.pusher
				.get_pushkeys(user)
				.ready_for_each(|push_key| {
					self.services
						.sending
						.send_pdu_push(pdu_id, user, push_key.to_owned())
						.expect("TODO: replace with future");
				})
				.await;
		}

		self.db
			.increment_notification_counts(pdu.room_id().unwrap(), notifies, highlights);
	}

	/// Handles PDU effects based on the type of incoming event.
	/// For redaction events, handles redacting. Memberships update the
	/// membership cache. Et cetera.
	async fn handle_pdu_effects(
		&self,
		pdu: &PduEvent,
		pdu_id: &RawPduId,
		short_room_id: ShortRoomId,
	) -> Result {
		let room_id = pdu.room_id().unwrap();
		match *pdu.kind() {
			| TimelineEventType::RoomRedaction => {
				use RoomVersionId::*;

				// TODO: support delayed redaction (MSC2815)
				let room_version_id = self.services.state.get_room_version(room_id).await?;
				match room_version_id {
					| V1 | V2 | V3 | V4 | V5 | V6 | V7 | V8 | V9 | V10 => {
						if let Some(redact_id) = pdu.redacts() {
							if self
								.services
								.state_accessor
								.user_can_redact(redact_id, pdu.sender(), room_id, false)
								.await?
							{
								self.redact_pdu(redact_id, pdu, short_room_id).await?;
							}
						}
					},
					| _ => {
						let content: RoomRedactionEventContent = pdu.get_content()?;
						if let Some(redact_id) = &content.redacts {
							if self
								.services
								.state_accessor
								.user_can_redact(redact_id, pdu.sender(), room_id, false)
								.await?
							{
								self.redact_pdu(redact_id, pdu, short_room_id).await?;
							}
						}
					},
				}
			},
			| TimelineEventType::RoomMember => {
				if let Some(state_key) = pdu.state_key() {
					// if the state_key fails
					let target_user_id = UserId::parse(state_key)
						.expect("This state_key was previously validated");

					// Update our membership info, we do this here incase a user is invited or
					// knocked and immediately leaves we need the DB to record the invite or
					// knock event for auth
					self.services
						.state_cache
						.update_membership(room_id, &target_user_id, pdu, true)
						.await?;
				}
			},
			| TimelineEventType::RoomMessage => {
				let content: ExtractBody = pdu.get_content()?;
				if let Some(body) = content.body {
					self.services.search.index_pdu(short_room_id, pdu_id, &body);
				}
			},
			| _ => {},
		}

		Ok(())
	}

	/// Adds relation data to the incoming event and events it relates to.
	async fn aggregate_relations(&self, pdu: &PduEvent, count2: Count) {
		// CONCERN: If we receive events with a relation out-of-order, we never write
		// their relation / thread. We need some kind of way to trigger when we receive
		// this event, and potentially a way to rebuild the table entirely.

		if let Ok(content) = pdu.get_content::<ExtractRelatesToEventId>() {
			if let Ok(related_pducount) = self.get_pdu_count(&content.relates_to.event_id).await {
				self.services
					.pdu_metadata
					.add_relation(count2, related_pducount);
			}
		}

		if let Ok(content) = pdu.get_content::<ExtractRelatesTo>() {
			match content.relates_to {
				| Relation::Reply(in_reply_to) => {
					// We need to do it again here, because replies don't have
					// event_id as a top level field
					if let Ok(related_pducount) =
						self.get_pdu_count(&in_reply_to.in_reply_to.event_id).await
					{
						self.services
							.pdu_metadata
							.add_relation(count2, related_pducount);
					}
				},
				| Relation::Thread(thread) => {
					self.services
						.threads
						.add_to_thread(&thread.event_id, pdu)
						.await
						.inspect_err(|e| {
							warn!(
								"Failed to add incoming event {} to thread {}: {e:?}",
								pdu.event_id(),
								thread.event_id
							);
						})
						.ok();
				},
				| _ => {}, // TODO: Aggregate other types
			}
		}
	}

	/// Determines if an appservice is interested in a particular event.
	async fn is_appservice_interested(
		&self,
		appservice: &RegistrationInfo,
		pdu: &PduEvent,
	) -> bool {
		let room_id = pdu.room_id().unwrap();
		let target = if *pdu.kind() == TimelineEventType::RoomMember {
			pdu.state_key().and_then(|sk| UserId::parse(sk).ok())
		} else {
			None
		};

		let (target_matches_sender, target_matches_namespace) = match target {
			| Some(target) => (
				appservice.registration.sender_localpart.as_str() == target.localpart(),
				appservice.users.is_match(target.as_str()),
			),
			| _ => (false, false),
		};
		let sender_matches_namespace = appservice.users.is_match(pdu.sender().as_str());

		if target_matches_sender || target_matches_namespace || sender_matches_namespace {
			return true;
		}

		let aliases = appservice.aliases.clone();
		self.services
			.alias
			.local_aliases_for_room(room_id)
			.ready_any(move |room_alias| aliases.is_match(room_alias.as_str()))
			.await
	}

	/// Notifies interested appservices of a new PDU.
	async fn send_to_interested_appservices(&self, pdu: &PduEvent, pdu_id: &RawPduId) {
		let interested_appservices = self
			.services
			.appservice
			.read()
			.await
			.values()
			.map(ToOwned::to_owned)  // TODO: is this to_owned expensive?
			.collect::<Vec<_>>();
		interested_appservices
			.stream()
			.broad_filter_map(|appservice| async move {
				self.is_appservice_interested(&appservice, pdu)
					.await
					.then_some(appservice)
			})
			.for_each_concurrent(None, |appservice| async move {
				self.services
					.sending
					.send_pdu_appservice(appservice.registration.id.clone(), *pdu_id)
					.inspect_err(|e| {
						warn!(
							"failed to send PDU {} to appservice {}: {e:?}",
							pdu.event_id(),
							appservice.registration.id
						);
					})
					.ok();
			})
			.await;
	}
}
