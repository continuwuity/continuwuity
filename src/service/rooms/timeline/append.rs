use std::{
	borrow::Borrow,
	collections::{BTreeMap, HashMap, HashSet},
	iter::once,
	sync::Arc,
};

use conduwuit::{
	Result, debug, debug_warn, is_equal_to,
	matrix::{Event, PduEvent},
	pdu::{Count, ShortRoomId},
	trace,
	utils::{
		IterStream, TryFutureExtExt,
		mutex_map::Guard,
		stream::{BroadbandExt, ReadyExt},
	},
	warn,
};
use conduwuit_core::{
	err, error,
	matrix::pdu::{PduCount, PduId, RawPduId, sticky},
	result::LogErr,
	utils::{self, stream::TryIgnore},
};
use futures::{FutureExt, StreamExt, TryFutureExt, pin_mut};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, EventId, OwnedEventId, OwnedRoomId, OwnedServerName,
	RoomId, RoomVersionId, ServerName, UserId,
	events::{
		AnySyncTimelineEvent, GlobalAccountDataEventType, TimelineEventType,
		push_rules::PushRulesEvent,
		room::{
			encrypted::Relation,
			member::{MembershipState, RoomMemberEventContent},
			redaction::RoomRedactionEventContent,
		},
	},
	push::{Action, Ruleset, Tweak},
	room_version_rules::RoomVersionRules,
	serde::Raw,
};

use super::{ExtractBody, ExtractRelatesTo, ExtractRelatesToEventId, RoomMutexGuard};
use crate::{
	appservice::RegistrationInfo,
	rooms::state_compressor::{CompressedState, HashSetCompressStateEvent},
};

impl super::Service {
	/// Append the incoming event setting the state snapshot to the state from
	/// the server that sent the event.
	pub async fn append_incoming_pdu<'a>(
		&'a self,
		pdu: &'a PduEvent,
		pdu_json: CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
		state_before: HashMap<u64, OwnedEventId>,
		should_soft_fail: bool,
		state_lock: &'a RoomMutexGuard,
	) -> Result<Option<RawPduId>> {
		let room_id = pdu.room_id_or_hash();

		let sb = state_before.iter().map(|(ssk, eid)| (ssk, eid.as_ref()));
		let state_ids_compressed: Arc<CompressedState> = self
			.services
			.state_compressor
			.compress_state_events(sb)
			.collect()
			.map(Arc::new)
			.await;

		// We append to state before appending the pdu, so we don't have a moment in
		// time with the pdu without it's state. This is okay because append_pdu can't
		// fail.
		self.services
			.state
			.set_event_state(&pdu.event_id, &room_id, state_ids_compressed)
			.await?;

		if should_soft_fail {
			// Nothing else to do with a soft-failed event.
			self.services
				.pdu_metadata
				.mark_event_soft_failed(pdu.event_id());
			return Ok(None);
		}

		// Per https://spec.matrix.org/unstable/server-server-api/#soft-failure, soft-failed events
		// are not added as forward extremities.
		// This also means we set the state here.
		// We do this BEFORE setting the extremities so that there's never a point in
		// time where we have fresh extremities referencing stale state.
		let mut extremities: Vec<_> = self
			.services
			.state
			.get_forward_extremities(&room_id)
			.collect()
			.await;
		extremities = self
			.progress_state_and_extremities(
				pdu,
				room_version_rules,
				state_before.clone(),
				extremities,
				state_lock,
			)
			.await?;

		let pdu_id = self
			.append_pdu(
				pdu,
				pdu_json,
				extremities.iter().map(Borrow::borrow),
				state_lock,
				&room_id,
			)
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

		Ok(Some(pdu_id))
	}

	/// Derives new room state from the incoming event and filters forward
	/// extremities accordingly. Does not set forward extremities.
	///
	/// Only call this function if the incoming PDU is not soft-failed or
	/// rejected.
	async fn progress_state_and_extremities(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		state_before: HashMap<u64, OwnedEventId>,
		forward_extremities: Vec<OwnedEventId>,
		state_lock: &Guard<OwnedRoomId, ()>,
	) -> Result<Vec<OwnedEventId>> {
		if incoming_pdu.state_key().is_some() {
			debug!("Event is a state-event. Deriving new room state");
			self.derive_new_state(incoming_pdu, room_version_rules, state_before, state_lock)
				.await?;
		}

		// Now we calculate the set of extremities this room has after the incoming
		// event has been applied. We start with the previous extremities
		trace!("Calculating extremities");
		let mut forward_extremities = forward_extremities
			.into_iter()
			.stream()
			.ready_filter(|event_id| {
				// Remove any that are referenced by this incoming event's prev_events
				!incoming_pdu.prev_events().any(is_equal_to!(event_id))
			})
			.broad_filter_map(|event_id| async move {
				// Only keep those extremities were not referenced yet
				self.services
					.pdu_metadata
					.is_event_referenced(&incoming_pdu.room_id_or_hash(), &event_id)
					.await
					.eq(&false)
					.then_some(event_id)
			})
			.collect::<Vec<_>>()
			.await;

		// An event with a child is not a leaf, but a room with no extremities cannot be
		// written to, so it is added anyway when nothing else remains.
		let has_child = self
			.services
			.pdu_metadata
			.is_event_referenced(&incoming_pdu.room_id_or_hash(), incoming_pdu.event_id())
			.await;
		if !has_child || forward_extremities.is_empty() {
			if has_child {
				warn!(
					"Adding {} as an extremity despite its child, the room has no other leaves",
					incoming_pdu.event_id()
				);
			}
			forward_extremities.push(incoming_pdu.event_id().to_owned());
		}

		debug!(
			"Retained {} extremities checked against {} prev_events",
			forward_extremities.len(),
			incoming_pdu.prev_events().count()
		);
		assert!(!forward_extremities.is_empty(), "resolved extremities cannot be empty");
		Ok(forward_extremities)
	}

	/// Derives a new room state by adding the incoming PDU to the state before
	/// it to create the state at, which then becomes the current room state.
	///
	/// The caller MUST ensure forward extremities are set appropriately,
	/// including this incoming pdu, either before or after calling this
	/// function. Failing to do so will result in an inconsistent current state
	/// cache, which may affect event authentication.
	async fn derive_new_state(
		&self,
		incoming_pdu: &PduEvent,
		room_version_rules: &RoomVersionRules,
		state_before: HashMap<u64, OwnedEventId>,
		state_lock: &Guard<OwnedRoomId, ()>,
	) -> Result {
		let room_id = incoming_pdu.room_id_or_hash();
		// We also add state after incoming event to the fork states
		let mut state_at_incoming_event = state_before;
		let shortstatekey = self
			.services
			.short
			.get_or_create_shortstatekey(
				&incoming_pdu.kind().to_string().into(),
				incoming_pdu.state_key().unwrap(),
			)
			.await;

		let event_id = incoming_pdu.event_id();
		state_at_incoming_event.insert(shortstatekey, event_id.to_owned());

		debug!("Resolving new room state");
		let new_room_state = self
			.services
			.event_handler
			.resolve_state(&room_id, room_version_rules, state_at_incoming_event)
			.await?;

		debug!("Forcing new room state");
		let HashSetCompressStateEvent { shortstatehash, added, removed } = self
			.services
			.state_compressor
			.save_state(&room_id, new_room_state)
			.await?;

		self.services
			.state
			.force_state(&room_id, shortstatehash, added, removed, state_lock)
			.await
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
		room_id: &'a RoomId,
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

		let count2 = PduCount::Normal(self.services.globals.next_count().unwrap());
		let pdu_id: RawPduId = PduId { shortroomid, shorteventid: count2 }.into();

		// Insert pdu
		self.db.append_pdu(&pdu_id, pdu, &pdu_json, count2).await;

		drop(insert_lock);

		// See if the event matches any known pushers via power level
		if *pdu.kind() != TimelineEventType::RoomCreate {
			tokio::join!(
				self.notify_local_users(pdu, &pdu_id, room_id),
				self.handle_pdu_effects(pdu, &pdu_id, room_id, shortroomid)
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

		self.services
			.read_receipt
			.private_read_set(room_id, pdu.sender(), count1);

		self.services
			.user
			.reset_notification_counts(pdu.sender(), room_id);

		self.send_to_interested_appservices(pdu, &pdu_id, room_id)
			.await;

		Ok(pdu_id)
	}

	/// Notifies local users of the incoming event with a power levels context.
	async fn notify_local_users(&self, pdu: &PduEvent, pdu_id: &RawPduId, room_id: &RoomId) {
		let power_levels = self
			.services
			.state_accessor
			.get_room_power_levels(room_id)
			.await;
		let mut push_targets: HashSet<_> = self
			.services
			.state_cache
			.active_local_users_in_room(room_id)
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
						if self
							.services
							.users
							.status(&target_user_id)
							.await
							.is_active()
						{
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

		let serialized: Raw<AnySyncTimelineEvent> = pdu.to_format();
		let room_joined_count = self.services.pusher.push_joined_count(room_id).await;

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

			let ctx = self
				.services
				.pusher
				.push_condition_ctx(user, power_levels.clone(), room_id, room_joined_count)
				.await;

			for action in rules_for_user.get_actions(&serialized, &ctx).await {
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
			.increment_notification_counts(room_id, notifies, highlights);
	}

	/// Handles PDU effects based on the type of incoming event.
	/// For redaction events, handles redacting. Memberships update the
	/// membership cache. Et cetera.
	async fn handle_pdu_effects(
		&self,
		pdu: &PduEvent,
		pdu_id: &RawPduId,
		room_id: &RoomId,
		short_room_id: ShortRoomId,
	) -> Result {
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

					// must be checked before the membership update, which is what
					// marks the server as being in the room
					let joining_server = self.joining_server(pdu, &target_user_id, room_id).await;

					// Update our membership info, we do this here incase a user is invited or
					// knocked and immediately leaves we need the DB to record the invite or
					// knock event for auth
					self.services
						.state_cache
						.update_membership(room_id, &target_user_id, pdu, true)
						.await?;

					if let Some(server) = joining_server {
						self.send_sticky_events_to_server(room_id, short_room_id, &server)
							.await;
					}
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

	/// The server of `target_user_id`, if this membership event brings it into
	/// the room for the first time.
	async fn joining_server(
		&self,
		pdu: &PduEvent,
		target_user_id: &UserId,
		room_id: &RoomId,
	) -> Option<OwnedServerName> {
		let server = target_user_id.server_name();
		if !self.services.config.allow_sticky_events
			|| self.services.globals.server_is_ours(server)
		{
			return None;
		}

		let content: RoomMemberEventContent = pdu.get_content().ok()?;
		if content.membership != MembershipState::Join {
			return None;
		}

		let already_joined = self
			.services
			.state_cache
			.server_in_room(server, room_id)
			.await;

		(!already_joined).then(|| server.to_owned())
	}

	/// MSC4354: a server which has just joined will not backfill far enough to
	/// see our unexpired sticky events, so push them to it.
	async fn send_sticky_events_to_server(
		&self,
		room_id: &RoomId,
		short_room_id: ShortRoomId,
		server: &ServerName,
	) {
		let now = utils::millis_since_unix_epoch();
		let oldest_sticky_ts = now.saturating_sub(sticky::MAX_DURATION_MS);

		let mut pdu_ids = Vec::new();
		let pdus = self.pdus_rev(room_id, None).ignore_err();

		pin_mut!(pdus);
		while let Some((count, pdu)) = pdus.next().await {
			if u64::from(pdu.origin_server_ts) < oldest_sticky_ts {
				break;
			}

			let is_ours = self.services.globals.user_is_local(pdu.sender());
			let is_sticky = pdu
				.sticky
				.as_deref()
				.is_some_and(|sticky| sticky::is_sticky(pdu.origin_server_ts, sticky, now));

			if is_ours && is_sticky {
				pdu_ids.push(RawPduId::from(PduId {
					shortroomid: short_room_id,
					shorteventid: count,
				}));
			}
		}

		// oldest first, as MSC4354 asks for creation order
		for pdu_id in pdu_ids.iter().rev() {
			self.services
				.sending
				.send_pdu_servers(once(server.to_owned()).stream(), pdu_id)
				.await
				.log_err()
				.ok();
		}
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
		room_id: &RoomId,
	) -> bool {
		if self
			.services
			.state_cache
			.appservice_in_room(room_id, appservice)
			.await
		{
			return true;
		}

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
		let room_matches_namespace = appservice.rooms.is_match(room_id.as_str());

		if target_matches_sender
			|| target_matches_namespace
			|| sender_matches_namespace
			|| room_matches_namespace
		{
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
	async fn send_to_interested_appservices(
		&self,
		pdu: &PduEvent,
		pdu_id: &RawPduId,
		room_id: &RoomId,
	) {
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
				self.is_appservice_interested(&appservice, pdu, room_id)
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
