use std::{
	collections::BTreeMap,
	time::{Duration, Instant},
};

use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_error, debug_info, debug_warn, defer, err, error,
	info, matrix::PartialPdu, result::DebugInspect, trace, utils::time::jitter, warn,
};
use futures::{
	FutureExt, StreamExt,
	future::{OptionFuture, try_join4},
};
use ruma::{
	CanonicalJsonValue, EventId, OwnedUserId, RoomId, ServerName, UserId,
	events::{
		TimelineEventType,
		room::member::{MembershipState, RoomMemberEventContent},
	},
};
use tokio::sync::mpsc;

use crate::rooms::timeline::{RawPduId, pdu_fits};

async fn should_rescind_invite(
	services: &crate::rooms::event_handler::Services,
	content: &mut BTreeMap<String, CanonicalJsonValue>,
	sender: &UserId,
	room_id: &RoomId,
) -> Result<Option<PduEvent>> {
	// We insert a bogus event ID since we can't actually calculate the right one
	content.insert("event_id".to_owned(), CanonicalJsonValue::String("$rescind".to_owned()));
	let pdu_event = serde_json::from_value::<PduEvent>(
		serde_json::to_value(&content).expect("CanonicalJsonObj is a valid JsonValue"),
	)
	.map_err(|e| err!("invalid PDU: {e}"))?;

	if pdu_event.room_id().is_none_or(|r| r != room_id)
		&& pdu_event.sender() != sender
		&& pdu_event.event_type() != &TimelineEventType::RoomMember
		&& pdu_event.state_key().is_none_or(|v| v == sender.as_str())
	{
		return Ok(None);
	}

	let target_user_id = UserId::parse(pdu_event.state_key().unwrap())?;
	if pdu_event
		.get_content::<RoomMemberEventContent>()?
		.membership
		!= MembershipState::Leave
	{
		return Ok(None); // Not a leave event
	}

	// Does the target user have a pending invite?
	let Ok(pending_invite_state) = services
		.state_cache
		.invite_state(&target_user_id, room_id)
		.await
	else {
		return Ok(None); // No pending invite, so nothing to rescind
	};
	for event in pending_invite_state {
		if event
			.get_field::<String>("type")?
			.is_some_and(|t| t == "m.room.member")
			|| event
				.get_field::<OwnedUserId>("state_key")?
				.is_some_and(|s| s == *target_user_id)
			|| event
				.get_field::<OwnedUserId>("sender")?
				.is_some_and(|s| s == *sender)
			|| event
				.get_field::<RoomMemberEventContent>("content")?
				.is_some_and(|c| c.membership == MembershipState::Invite)
		{
			return Ok(Some(pdu_event));
		}
	}

	Ok(None)
}

impl super::Service {
	/// When receiving an event one needs to:
	/// 0. Check the server is in the room
	/// 1. Skip the PDU if we already know about it
	/// 1.1. Remove unsigned field
	/// 2. Check signatures, otherwise drop
	/// 3. Check content hash, redact if doesn't match
	/// 4. Fetch any missing auth events doing all checks listed here starting
	///    at 1. These are not timeline events
	/// 5. Reject "due to auth events" if can't get all the auth events or some
	///    of the auth events are also rejected "due to auth events"
	/// 6. Reject "due to auth events" if the event doesn't pass auth based on
	///    the auth events
	/// 7. Persist this event as an outlier
	/// 8. If not timeline event: stop
	/// 9. Fetch any missing prev events doing all checks listed here starting
	///    at 1. These are timeline events
	/// 10. Fetch missing state and auth chain events by calling `/state_ids` at
	///     backwards extremities doing all the checks in this list starting at
	///     1. These are not timeline events
	/// 11. Check the auth of the event passes based on the state of the event
	/// 12. Ensure that the state is derived from the previous current state
	///     (i.e. we calculated by doing state res where one of the inputs was a
	///     previously trusted set of state, don't just trust a set of state we
	///     got from a remote)
	/// 13. Use state resolution to find new room state
	/// 14. Check if the event passes auth based on the "current state" of the
	///     room, if not soft fail it
	#[tracing::instrument(
		name = "pdu",
		skip_all,
		fields(%room_id, %event_id),
	)]
	pub async fn handle_incoming_pdu<'a>(
		&self,
		origin: &'a ServerName,
		room_id: &'a RoomId,
		event_id: &'a EventId,
		value: BTreeMap<String, CanonicalJsonValue>,
		is_timeline_event: bool,
	) -> Result<Option<RawPduId>> {
		// 1. Skip the PDU if we already have it as a timeline event
		if let Ok(pdu_id) = self.services.timeline.get_pdu_id(event_id).await {
			return Ok(Some(pdu_id));
		}
		if !pdu_fits(&mut value.clone()) {
			warn!(
				"dropping incoming PDU {event_id} in room {room_id} from {origin} because it \
				 exceeds 65535 bytes or is otherwise too large."
			);
			return Err!(Request(TooLarge("PDU is too large")));
		}
		trace!(
			"processing incoming PDU from {origin} for room {room_id} with event id {event_id}"
		);

		// 1.1 Check we even know about the room
		let meta_exists = self.services.metadata.exists(room_id).map(Ok);

		// 1.2 Check if the room is disabled
		let is_disabled = self.services.metadata.is_disabled(room_id).map(Ok);

		// 1.3.1 Check room ACL on origin field/server
		let origin_acl_check = self.acl_check(origin, room_id);

		// 1.3.2 Check room ACL on sender's server name
		let sender: OwnedUserId = value
			.get("sender")
			.and_then(|v| v.as_str())
			.ok_or_else(|| err!("No sender in object"))
			.and_then(|v| Ok(UserId::parse(v)?))
			.map_err(|e| err!(Request(BadJson("PDU does not have a valid sender key: {e}"))))?;

		let sender_acl_check: OptionFuture<_> = sender
			.server_name()
			.ne(origin)
			.then(|| self.acl_check(sender.server_name(), room_id))
			.into();

		let (meta_exists, is_disabled, (), ()) = try_join4(
			meta_exists,
			is_disabled,
			origin_acl_check,
			sender_acl_check.map(|o| o.unwrap_or(Ok(()))),
		)
		.await
		.inspect_err(
			|e| debug_error!(%origin, "failed to handle incoming PDU {event_id}: {e}"),
		)?;

		if is_disabled {
			return Err!(Request(Forbidden(
				"Federation of this room is disabled by this server."
			)));
		}

		if !self
			.services
			.state_cache
			.server_in_room(self.services.globals.server_name(), room_id)
			.await
		{
			// Is this a federated invite rescind?
			// copied from https://github.com/element-hq/synapse/blob/7e4588a/synapse/handlers/federation_event.py#L255-L300
			if value.get("type").and_then(|t| t.as_str()) == Some("m.room.member") {
				if let Some(pdu) =
					should_rescind_invite(&self.services, &mut value.clone(), &sender, room_id)
						.await?
				{
					debug_info!(
						"Invite to {room_id} appears to have been rescinded by {sender}, \
						 marking as left"
					);
					self.services
						.state_cache
						.mark_as_left(&sender, room_id, Some(pdu))
						.await;
					return Ok(None);
				}
			}
			info!(
				%origin,
				%room_id,
				"Dropping inbound PDU for room we aren't participating in"
			);
			return Err!(Request(NotFound("This server is not participating in that room.")));
		}

		if !meta_exists {
			return Err!(Request(NotFound("Room is unknown to this server")));
		}

		// Fetch create event
		let create_event = &self
			.services
			.state_accessor
			.get_room_create_event(room_id)
			.await;

		let start_time = Instant::now();
		self.federation_handletime
			.write()
			.insert(room_id.into(), (event_id.to_owned(), start_time));

		defer! {{
			self.federation_handletime
				.write()
				.remove(room_id);
		}}

		let (incoming_pdu, val) = self
			.handle_outlier_pdu(origin, create_event, event_id, room_id, value)
			.await?;

		// 8. if not timeline event: stop
		if !is_timeline_event {
			return Ok(None);
		}

		// Skip events sent before we joined (they need to be persisted as backfilled
		// events, not timeline events, which is handled elsewhere).
		let first_ts_in_room = self
			.services
			.timeline
			.first_pdu_in_room(room_id)
			.await?
			.origin_server_ts();
		if incoming_pdu.origin_server_ts() < first_ts_in_room {
			return Ok(None);
		}

		// 9. Fetch any missing prev events doing all checks listed here starting at 1.
		//    These are timeline events

		debug!("Fetching and persisting any missing prev events");
		Box::pin(self.fetch_prevs(
			room_id,
			create_event,
			&incoming_pdu,
			origin,
			first_ts_in_room,
		))
		.await
		.debug_inspect_err(|e| {
			error!("Failed to fetch and persist incoming event's prev_events: {e:?}");
		})?;

		let is_dummy_event = incoming_pdu.event_type().to_string() == "org.matrix.dummy_event";

		// Done with prev events, now handling the incoming event
		let pdu_id = Box::pin(self.upgrade_outlier_to_timeline_pdu(
			incoming_pdu,
			val,
			create_event,
			origin,
			room_id,
		))
		.await?;

		let extremities_count = self
			.services
			.state
			.get_forward_extremities(room_id)
			.count()
			.await;

		self.maybe_squash_extremities(room_id, extremities_count, is_dummy_event)
			.await;

		Ok(pdu_id)
	}

	async fn maybe_squash_extremities(
		&self,
		room_id: &RoomId,
		extremities_count: usize,
		is_dummy_event: bool,
	) {
		let (tx, fut) = {
			if let Some(tx) = self.extremity_squashers.read().get(room_id)
				&& !tx.is_closed()
			{
				(tx.clone(), None)
			} else {
				let mut map = self.extremity_squashers.upgradable_read();

				if let Some(tx) = map.get(room_id)
					&& !tx.is_closed()
				{
					(tx.clone(), None)
				} else {
					let (tx, rx) = mpsc::channel(100);
					map.with_upgraded(|map| map.insert(room_id.to_owned(), tx.clone()));

					(tx, Some(self.spawn_squasher(room_id, rx)))
				}
			}
		};

		if let Some(fut) = fut {
			fut.await;
		}
		let _ = tx.try_send((extremities_count, is_dummy_event));
	}

	async fn spawn_squasher(&self, room_id: &RoomId, mut rx: mpsc::Receiver<(usize, bool)>) {
		let Some(service) = self.me.upgrade() else {
			return;
		};
		let room_id = room_id.to_owned();

		self.services.server.runtime().spawn(async move {
			let mut latest_extremity_count = None;
			let mut non_dummy_event = false;

			let mut closing = false;

			let waker = tokio::time::sleep(jitter(Duration::from_mins(2), -25.0..=25.0));
			tokio::pin!(waker);

			loop {
				tokio::select! {
					msg = rx.recv() => {
						if let Some((extremities_count, is_dummy_event)) = msg {
							latest_extremity_count = Some(extremities_count);
							non_dummy_event = non_dummy_event || !is_dummy_event;
							#[allow(clippy::arithmetic_side_effects)]
							waker.as_mut().reset(tokio::time::Instant::now() + jitter(Duration::from_mins(1), -50.0..=50.0));
						} else {
							{let mut map = service.extremity_squashers.write();
							if let Some(tx) = map.get(&room_id) && tx.is_closed() {
								map.remove(&room_id);
							}}

							if let Some(count) = latest_extremity_count {
								if non_dummy_event && count >= service.services.server.config.dummy_event_threshold.into() {
									Self::squash_extremities(&service, &room_id, count).await;
								}
							}
							break;
						}
					}
					() = &mut waker, if !closing => {
						if let Some(count) = latest_extremity_count {
							if non_dummy_event && count >= service.services.server.config.dummy_event_threshold.into() {
								Self::squash_extremities(&service, &room_id, count).await;
							}
							latest_extremity_count = None;
							non_dummy_event = false;
							#[allow(clippy::arithmetic_side_effects)]
							waker.as_mut().reset(tokio::time::Instant::now() + Duration::from_mins(2));
						} else {
							rx.close();
							closing = true;
						}
					}
					() = service.server_shutdown.notified(), if !closing => {
						rx.close();
						closing = true;
					}
				}
			}
		});
	}

	async fn squash_extremities(&self, room_id: &RoomId, extremities_count: usize) {
		debug_warn!(
			%extremities_count,
			threshold=%self.services.server.config.dummy_event_threshold,
			"Attempting to squash extremities after upgrading pdu"
		);
		// Try to send a dummy event to squash extremities. See issue #1844
		let power_levels = self
			.services
			.state_accessor
			.get_room_power_levels(room_id)
			.await;
		let mut local_users = self.services.state_cache.local_users_in_room(room_id);
		while let Some(user_id) = local_users.next().await {
			if !power_levels.user_can_send_message(&user_id, "org.matrix.dummy_event".into()) {
				trace!(%user_id, "user does not have power level to send dummy event, skipping");
				continue;
			}
			let state_lock = self.services.state.mutex.lock(room_id).await;
			if self
				.services
				.timeline
				.build_and_append_pdu(
					PartialPdu {
						event_type: "org.matrix.dummy_event".into(),
						..PartialPdu::default()
					},
					&user_id,
					Some(room_id),
					&state_lock,
				)
				.await
				.inspect(|_| debug!(sender=%user_id, "Successfully sent a dummy event"))
				.inspect_err(
					|e| debug!(sender=%user_id, ?e, "Failed to send a dummy event via user"),
				)
				.is_ok()
			{
				break;
			}
		}
	}
}
