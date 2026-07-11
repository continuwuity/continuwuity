use std::time::Instant;

use conduwuit::{
	Err, Result, debug, debug_info, debug_warn, is_true,
	matrix::{Event, PduEvent},
	trace,
};
use ruma::{CanonicalJsonObject, RoomId, ServerName, events::StateEventType};
use tokio::join;

use super::get_room_version_rules;
use crate::rooms::timeline::RawPduId;

impl super::Service {
	#[tracing::instrument(name="upgrade_outlier", skip_all, fields(event_id=%incoming_pdu.event_id()))]
	pub(super) async fn upgrade_outlier_to_timeline_pdu(
		&self,
		incoming_pdu: PduEvent,
		mut val: CanonicalJsonObject,
		create_event: &PduEvent,
		origin: &ServerName,
		room_id: &RoomId,
	) -> Result<Option<RawPduId>> {
		let (pduid, rejected, soft_failed) = join!(
			self.services.timeline.get_pdu_id(incoming_pdu.event_id()),
			self.services
				.pdu_metadata
				.is_event_rejected(incoming_pdu.event_id()),
			self.services
				.pdu_metadata
				.is_event_soft_failed(incoming_pdu.event_id())
		);
		if let Ok(id) = pduid {
			trace!(event_id=%incoming_pdu.event_id(), "Skipping upgrade of already upgraded PDU");
			return Ok(Some(id));
		} else if rejected {
			return Err!(Request(Forbidden(debug_info!("Event has been rejected"))));
		} else if soft_failed {
			// Soft-failed events cannot be promoted.
			return Err!(Request(Forbidden(debug_info!("Event has been soft-failed"))));
		}

		// These should never happen, but they're good last-minute sanity checks to
		// ensure we never promote totally illegal events.
		assert_eq!(
			*create_event.kind(),
			StateEventType::RoomCreate.into(),
			"tried to upgrade a PDU with a create_event that is not a room create event"
		);
		assert_eq!(
			incoming_pdu.room_id_or_hash(),
			*room_id,
			"room ID mismatch: PDU room ID differs from parameter"
		);

		debug!(
			event_id = %incoming_pdu.event_id,
			"Upgrading PDU from outlier to timeline"
		);
		let timer = Instant::now();
		let min_depth = self.services.metadata.get_mindepth(room_id).await;
		let room_version_rules = get_room_version_rules(create_event)?;

		// We now need to resolve the state before the event so that we can perform PDU
		// check 5 (event auth passes based on state before the event). To do this, we
		// either need to have all the prev events locally, or ask a remote server
		// for the state at the event.
		let (passes_state_before, state_before) = self
			.state_before_check_5(&incoming_pdu, &room_version_rules, create_event, origin)
			.await?;

		if !passes_state_before {
			self.reject_and_persist(incoming_pdu.event_id(), &val);
			return Err!(Request(Forbidden(debug_warn!(
				"Event authorisation fails based on the state before the event"
			))));
		}

		// Now that we know the event passes both self-authentication, and
		// authentication based on the state before the event, we need to check that it
		// passes based on the *current* room state (state across all forward
		// extremities). If it doesn't, we accept it, but soft-fail it, and this
		// prevents it being promoted.

		// We lock the room here to prevent the current state from changing beneath us
		// mid-check.
		trace!(
			room_id = %room_id,
			"Locking the room"
		);
		let state_lock = self.services.state.mutex.lock(room_id).await;
		let passes_current_state = self
			.current_state_check_6(&incoming_pdu, &room_version_rules, create_event)
			.await
			.inspect(|passes| {
				if !*passes {
					debug_warn!(
						"Event authorisation fails based on the current room state - will be \
						 soft-failed"
					);
				}
			})?;

		// Determine whether this PDU should be soft-failed.
		// If the auth check failed, invariably yes. Otherwise, only if the user isn't
		// allowed to redact the target event (if any).
		let mut should_soft_fail =
			match (passes_current_state, incoming_pdu.redacts_id(&room_version_rules)) {
				| (false, _) => true,
				| (true, None) => false,
				| (true, Some(redact_id)) => self
					.services
					.state_accessor
					.user_can_redact(&redact_id, incoming_pdu.sender(), room_id, true)
					.await
					.is_ok_and(is_true!()),
			};

		if !should_soft_fail {
			// Now we can perform check 7, which is ensuring the event passes policy server
			// checks.
			// We explicitly only do this if we aren't already going to soft-fail the event,
			// since the policy server refusing this event also soft-fails it.
			debug!(event_id = %incoming_pdu.event_id, "Checking policy server for event");
			should_soft_fail = !self
				.policy_server_check_7(&incoming_pdu, &mut val, &room_version_rules)
				.await
				.inspect(|passes| {
					if !*passes {
						debug_warn!(
							"Event did not pass the policy server check and will be soft-failed"
						);
					}
				})?;

			// TODO: this is supposed to hide redactions from policy servers and janitorial
			// bots, however, for full efficacy it also needs to hide redactions for
			// unknown events. This needs to be investigated at a later time.
			if let Some(redact_id) = incoming_pdu.redacts_id(&room_version_rules) {
				debug!(
					redact_id = %redact_id,
					"Checking if redaction is for a soft-failed/rejected event"
				);
				if !self
					.services
					.pdu_metadata
					.is_event_accepted(&redact_id)
					.await
				{
					debug_info!(
						"Soft-failing valid redaction because it targets a non-accepted event"
					);
					should_soft_fail = true;
				}
			}
		}

		// The PDU has now passed all checks! We can now promote it (or soft-fail it if
		// the verdict is such).
		trace!("Appending pdu to timeline");
		let pdu_id = self
			.services
			.timeline
			.append_incoming_pdu(
				&incoming_pdu,
				val,
				&room_version_rules,
				state_before,
				should_soft_fail,
				&state_lock,
			)
			.await?;

		if should_soft_fail {
			debug_info!(
				elapsed = ?timer.elapsed(),
				event_id = %incoming_pdu.event_id,
				"Event was soft failed"
			);
		} else {
			debug_info!(
				elapsed = ?timer.elapsed(),
				"Accepted",
			);
		}

		// Event has passed all auth/stateres checks
		drop(state_lock);
		if incoming_pdu.depth > min_depth && incoming_pdu.state_key().is_some() {
			self.services
				.metadata
				.set_mindepth(room_id, incoming_pdu.depth.into());
			trace!("Increased room's min depth from {} to {}", min_depth, incoming_pdu.depth);
		}

		Ok(pdu_id)
	}
}
