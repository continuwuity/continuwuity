use std::{
	collections::BTreeMap,
	time::{Duration, Instant},
};

use conduwuit::{
	Err, Event, Result, debug, debug_error, debug_warn, defer, error, matrix::PartialPdu,
	result::DebugInspect, trace, utils::time::jitter,
};
use futures::{FutureExt, StreamExt, future::try_join3};
use ruma::{CanonicalJsonValue, EventId, RoomId, ServerName, UserId};
use tokio::sync::mpsc;

use crate::rooms::timeline::RawPduId;

impl super::Service {
	/// Handles an incoming PDU from federation.
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
		// Skip the PDU if we already have it as a timeline event. We still re-process
		// outliers in this scenario.
		if let Ok(pdu_id) = self.services.timeline.get_pdu_id(event_id).await {
			return Ok(Some(pdu_id));
		}
		trace!(
			"processing incoming PDU from {origin} for room {room_id} with event id {event_id}"
		);

		// If this is a membership state event for a local user, we will be interested
		// in this event even if we aren't in the room. This allows us to process things
		// like revoking invites over federation, without being in the room.
		let is_interesting_member_event = value.get("type").and_then(|t| t.as_str())
			== Some("m.room.member")
			&& value
				.get("state_key")
				.and_then(|s| s.as_str())
				.and_then(|s| UserId::parse(s).ok())
				.is_some_and(|u| self.services.globals.user_is_local(&u));

		let (room_exists, is_disabled, ()) = try_join3(
			self.services.metadata.exists(room_id).map(Ok),
			self.services.metadata.is_disabled(room_id).map(Ok),
			self.acl_check(origin, room_id),
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

		if !room_exists && !is_interesting_member_event {
			if is_interesting_member_event {
				// TODO: handle interesting membership events where we aren't in
				// the room
			}
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

		// If this is not a timeline event, stop now, as we don't want to de-outlier it.
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

		// Fetch any missing prev events doing all checks listed here starting at 1.
		// These are timeline events.
		// TODO: This part needs to be done in a background queue somewhere.

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

		let is_dummy_event = incoming_pdu.event_type().to_string() == "org.matrix.dummy_event"
			&& incoming_pdu.state_key().is_none();

		// Done with prev events, now we can handle promoting the PDU
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

	/// Conditionally starts an extremity squasher. If there is no waiting
	/// extremity squasher, a new one is created. Otherwise, the existing one is
	/// pinged.
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

	/// Spawns an extremity squasher with the given room and receiver channel.
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
							let sleep_duration = if extremities_count >= 20 {
								// Skip the original sleep duration and send in the next 3-7 seconds as the number of extremities has grown beyond what one squash can reasonably reduce. We still jitter here in case we receive more events in that time that reduce the number anyway, and to account for other servers sending the same squashes.
								jitter(Duration::from_secs(5), -50.0..=50.0)
							} else {
								jitter(Duration::from_mins(1), -50.0..=50.0)
							};
							#[allow(clippy::arithmetic_side_effects)]
							waker.as_mut().reset(tokio::time::Instant::now() + sleep_duration);
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

	/// Squashes extremities in a room by sending dummy events (empty events
	/// that are hidden from clients) to the room. It will only send ONE dummy
	/// event to squash. If there are more than 20 extremities, multiple calls
	/// to `squash_extremities` will be required.
	/// Sending the dummy event will be attempted by iterating over each local
	/// user currently joined to the room (including deactivated users) until
	/// either one of them successfully builds and appends a dummy event PDU, or
	/// there are no more users to try.
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
				return;
			}
		}
		debug_warn!("Unable to squash extremities using any local user");
	}
}
