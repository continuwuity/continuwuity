use std::{
	borrow::Borrow,
	collections::{BTreeMap, HashMap},
	sync::Arc,
	time::Instant,
};

use assign::assign;
use conduwuit::{
	Err, Event, PduEvent, Result, debug, debug_error, debug_info, err, error, info,
	matrix::StateKey,
	state_res,
	state_res::EventTypeExt,
	trace,
	utils::{
		IterStream,
		stream::{BroadbandExt, WidebandExt, automatic_width},
	},
	warn,
};
use futures::{StreamExt, future::ready};
use ruma::{
	CanonicalJsonObject, EventId, OwnedEventId, OwnedRoomId, RoomId, RoomVersionId, ServerName,
	UserId,
	api::{
		error::{ErrorKind, IncompatibleRoomVersionErrorData},
		federation::membership::{create_join_event, prepare_join_event},
	},
	events::{StateEventType, room::member::MembershipState},
	room_version_rules::RoomVersionRules,
};
use serde_json::value::to_raw_value;

use crate::rooms::{
	event_handler::{DagBuilderTree, build_local_dag},
	membership::validate_remote_member_event_stub,
	state::RoomMutexGuard,
	state_compressor::{CompressedState, HashSetCompressStateEvent},
};

type StateTypeKey = (StateEventType, StateKey);

pub enum MakeJoinResult {
	/// A make_join request returned and the response was validated
	/// successfully.
	Success((CanonicalJsonObject, RoomVersionId)),
	/// A make_join response indicated further attempts to join should not be
	/// made (e.g. Forbidden).
	Fatal(conduwuit::Error),
	/// A make_join request failed (e.g. remote server unreachable, or unable to
	/// fulfill restricted join requirements), but another join attempt through
	/// another remote should be attempted.
	Retry,
}

fn state_key_from_json(obj: &CanonicalJsonObject) -> Option<(StateEventType, StateKey)> {
	let event_type = StateEventType::from(obj.get("type")?.as_str()?);
	let state_key = StateKey::from_str(obj.get("state_key")?.as_str()?);
	Some((event_type, state_key))
}

impl super::Service {
	/// Performs `POST /_matrix/federation/v1/make_join/{room_id}/{user_id}`.
	///
	/// If the request is successful, the resulting template is validated to
	/// ensure the remote server isn't returning a bad event, and then the
	/// template and room version are returned.
	/// If validation fails, `Ok(None)` is returned, in
	///
	/// If the request is unsuccessful, either an `Err` or `Ok(None)` will be
	/// returned, depending on the reason for the failure. If the join loop
	/// should terminate (for example, a `M_FORBIDDEN` error was returned,
	/// indicating we cannot join), then an `Err` will be returned. If the join
	/// loop should continue because the request failed due to conditional
	/// problems (e.g. server offline or unable to grant join), then `Ok(None)`
	/// will be returned.
	pub async fn make_join(
		&self,
		room_id: &RoomId,
		user_id: &UserId,
		via: &ServerName,
	) -> MakeJoinResult {
		assert!(
			self.services.globals.user_is_local(user_id),
			"can only make_join for local users"
		);

		let request = assign!(
			prepare_join_event::v1::Request::new(room_id.to_owned(), user_id.to_owned()),
			{ ver: self.services.server.supported_room_versions().collect() }
		);

		info!("Asking {via} for make_join");
		let make_join_response = self
			.services
			.sending
			.send_federation_request(via, request)
			.await
			.inspect(|_| info!("{via} finished make_join successfully"))
			.inspect_err(|e| debug_info!("{via} failed to make_join: {e:?}"));

		let make_join_response = match make_join_response {
			| Ok(r) => r,
			| Err(e) => return Self::handle_make_join_error(room_id, via, e),
		};

		// Make sure the remote server hasn't sent us garbage
		let Ok(template) = serde_json::from_str(make_join_response.event.get())
			.inspect_err(|e| warn!("{via} returned an invalid membership template: {e}"))
		else {
			return MakeJoinResult::Retry;
		};
		if let Err(e) =
			validate_remote_member_event_stub(&MembershipState::Join, user_id, room_id, &template)
		{
			warn!("{via} returned an illegal membership template event: {e:?}");
			return MakeJoinResult::Retry;
		}

		MakeJoinResult::Success((
			template,
			make_join_response.room_version.unwrap_or(RoomVersionId::V1),
		))
	}

	/// Handles an error response from make_join, determining whether it should
	/// be fatal (and returning `MakeJoinResult::Fatal(e)`), or
	/// temporary/situational and returning `MakeJoinResult::Retry`.
	fn handle_make_join_error(
		room_id: &RoomId,
		via: &ServerName,
		e: conduwuit::Error,
	) -> MakeJoinResult {
		match e.kind() {
			| ErrorKind::UnableToAuthorizeJoin => {
				info!(
					"{via} was unable to verify the joining user satisfied restricted join \
					 requirements: {e}."
				);
				MakeJoinResult::Retry
			},
			| ErrorKind::UnableToGrantJoin => {
				info!(
					"{via} believes the joining user satisfies restricted join rules, but is \
					 unable to authorise a join for us."
				);
				MakeJoinResult::Retry
			},
			| ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData {
				room_version,
				..
			}) => {
				warn!(
					"{via} reports the room we are trying to join is v{room_version}, which we \
					 do not support."
				);
				MakeJoinResult::Fatal(e)
			},
			| ErrorKind::Forbidden => {
				warn!("{via} refuses to let us join: {e}.");
				MakeJoinResult::Fatal(e)
			},
			| ErrorKind::NotFound => {
				info!("{via} does not know about {room_id}: {e}.");
				MakeJoinResult::Retry
			},
			| ErrorKind::Unknown if e.status_code().is_server_error() => {
				match e.status_code() {
					| http::StatusCode::BAD_GATEWAY | http::StatusCode::SERVICE_UNAVAILABLE =>
						info!("{via} is unavailable: {e}"),
					| http::StatusCode::GATEWAY_TIMEOUT =>
						info!("{via} timed out while handling make_join: {e}"),
					| _ => info!("{via} encountered an internal server error: {e}."),
				}
				MakeJoinResult::Retry
			},
			| _ => {
				info!("{via} unexpectedly failed to make_join: {e}.");
				MakeJoinResult::Retry
			},
		}
	}

	pub async fn send_join(
		&self,
		room_id: OwnedRoomId,
		event_id: OwnedEventId,
		event: &CanonicalJsonObject,
		via: &ServerName,
		room_version_rules: &RoomVersionRules,
		state_lock: &RoomMutexGuard,
	) -> Result<Option<()>> {
		let send_join_request = create_join_event::v2::Request::new(
			room_id.clone(),
			event_id.clone(),
			self.services
				.sending
				.convert_to_outgoing_federation_event(event.clone())
				.await,
		);

		info!("Asking {via} for send_join in room {room_id}");
		let Ok(send_join_response) = self
			.services
			.sending
			.send_slow_federation_request(via, send_join_request)
			.await
			.inspect(|resp| {
				info!(
					"{via} finished send_join successfully, returning {} state events and {} \
					 authentication events.",
					resp.room_state.state.len(),
					resp.room_state.auth_chain.len(),
				);
			})
			.inspect_err(|e| debug_error!("{via} failed to send_join: {e:?}"))
		else {
			return Ok(None); // Try another server
		};
		self.services
			.short
			.get_or_create_shortroomid(&room_id)
			.await;

		info!("Acquiring server signing keys for room state events");
		self.services
			.server_keys
			.acquire_events_pubkeys(
				send_join_response
					.room_state
					.state
					.iter()
					.chain(send_join_response.room_state.auth_chain.iter()),
			)
			.await;

		info!("Building state map");
		let (create_event, untrusted_state_before) = match self
			.build_state_map(&send_join_response, &room_id, room_version_rules)
			.await
		{
			| Ok(result) => result,
			| Err(e) => {
				error!("Failed to build state map: {e}");
				// This usually happens when the remote server sends a malformed response (e.g.
				// forgotten m.room.create event). In this case, we might succeed with another
				// server, so we will return Ok(None) to encourage the caller to re-attempt.
				return Ok(None);
			},
		};

		info!(
			"Handling returned room state authentication events ({} events)",
			send_join_response.room_state.auth_chain.len()
		);
		let auth_map = self
			.process_send_join_auth_chain(
				&send_join_response,
				&room_id,
				room_version_rules,
				&create_event,
			)
			.await?;

		info!("Handling returned room state ({} events)", untrusted_state_before.len());
		let untrusted_state_before_by_id = untrusted_state_before
			.values()
			.map(|value| {
				(
					value
						.get("event_id")
						.and_then(|v| v.as_str())
						.and_then(|v| EventId::parse(v).ok())
						.expect("We inserted event_id during parsing"),
					value,
				)
			})
			.collect::<HashMap<_, _>>();
		let state_before = self
			.process_send_join_state(
				untrusted_state_before_by_id,
				&auth_map,
				room_version_rules,
				&create_event,
			)
			.await?;
		let state_before_by_id = state_before
			.values()
			.map(|value| (value.event_id().to_owned(), value))
			.collect::<HashMap<_, _>>();

		info!("Authorizing join event");
		let final_membership_event =
			if let Some(event) = send_join_response.room_state.event.as_ref() {
				event
			} else {
				&to_raw_value(&event)
					.expect("must be able to convert CanonicalJsonObject to RawJsonValue")
			};

		let (membership_pdu, membership_pdu_json) = self
			.process_send_join_membership_event(
				final_membership_event,
				&event_id,
				&state_before,
				&state_before_by_id,
				room_version_rules,
			)
			.await?;

		// We've successfully joined now, and can update our local state cache.
		info!("Compressing resolved state");
		let compressed_state_map = state_before
			.into_iter()
			.stream()
			.broad_filter_map(|(key, pdu)| async move {
				let ssk = self
					.services
					.short
					.get_or_create_shortstatekey(&key.0, key.1.as_str())
					.await;
				Some((ssk, pdu.event_id.clone()))
			})
			.collect::<Vec<_>>()
			.await;
		let compressed: CompressedState = self
			.services
			.state_compressor
			.compress_state_events(
				compressed_state_map
					.iter()
					.map(|(ssk, eid)| (ssk, eid.borrow())),
			)
			.collect()
			.await;

		debug!("Saving compressed state");
		let HashSetCompressStateEvent {
			shortstatehash: statehash_before_join,
			added,
			removed,
		} = self
			.services
			.state_compressor
			.save_state(&room_id, Arc::new(compressed))
			.await?;

		debug!("Updating state cache");
		self.services
			.state
			.force_state(&room_id, statehash_before_join, added, removed, state_lock)
			.await?;

		let statehash_after_join = self
			.services
			.state
			.append_to_state(&membership_pdu, &room_id)
			.await?;

		debug!("Promoting membership event");
		self.services
			.timeline
			.append_pdu(
				&membership_pdu,
				membership_pdu_json,
				std::iter::once(membership_pdu.event_id()),
				state_lock,
				&room_id,
			)
			.await
			.inspect(|_| info!("Finished joining {room_id}"))?;
		self.services
			.metadata
			.maybe_set_mindepth(&room_id, membership_pdu.depth.into())
			.await;

		info!("Setting final room state for new room");
		// We set the room state after inserting the pdu, so that we never have a moment
		// in time where events in the current room state do not exist
		self.services
			.state
			.set_room_state(&room_id, statehash_after_join, state_lock);

		Ok(Some(()))
	}

	async fn build_state_map(
		&self,
		response: &create_join_event::v2::Response,
		room_id: &RoomId,
		room_version_rules: &RoomVersionRules,
	) -> Result<(PduEvent, HashMap<StateTypeKey, CanonicalJsonObject>)> {
		// NOTE: this is untrusted as we haven't performed any of the PDU checks yet.
		// Events in this map should not be persisted blindly.
		let untrusted_state_before = response
			.room_state
			.state
			.iter()
			.stream()
			.wide_filter_map(async |pdu| {
				let (event_room_id, event_id, mut value) = self
					.services
					.event_handler
					.parse_incoming_pdu(pdu, Some(room_version_rules))
					.await
					.inspect_err(|e| warn!("Invalid PDU in room state (dropping): {e:?}"))
					.ok()?;
				value.insert("event_id".to_owned(), event_id.as_str().into());
				if event_room_id != room_id {
					warn!(%event_id, expected=%room_id, actual=%event_room_id, "PDU in room state belongs to a different room (dropping)");
					return None;
				}
				if self.services.pdu_metadata.is_event_rejected(&event_id).await {
					// Rejection will be re-evaluated later
					trace!(%event_id, "Un-rejecting event");
					self.services.pdu_metadata.unmark_event_rejected(&event_id);
				}
				Some((state_key_from_json(&value)?, value))
			})
			.collect::<HashMap<(StateEventType, StateKey), CanonicalJsonObject>>()
			.await;

		let create_event: PduEvent = serde_json::from_value(serde_json::to_value(
			untrusted_state_before
				.get(&StateEventType::RoomCreate.with_state_key(""))
				.ok_or_else(|| {
					err!("Room state returned from send_join did not contain a room create event")
				})?,
		)?)?;

		Ok((create_event, untrusted_state_before))
	}

	async fn process_send_join_auth_chain(
		&self,
		response: &create_join_event::v2::Response,
		room_id: &RoomId,
		room_version_rules: &RoomVersionRules,
		create_event: &PduEvent,
	) -> Result<BTreeMap<OwnedEventId, PduEvent>> {
		let auth_chain_timing_start = Instant::now();
		let unauthed_auth_chain = response
			.room_state
			.auth_chain
			.iter()
			.stream()
			.broad_filter_map(|value| async move {
				self
					.services
					.event_handler
					.parse_incoming_pdu(
						&to_raw_value(&value)
							.expect("CanonicalJsonObject just convert to RawJsonValue"),
						Some(room_version_rules),
					)
					.await
					.inspect_err(|e| warn!("Invalid PDU in send_join auth chain (dropping): {e:?}"))
					.ok()
			})
			.broad_filter_map(|(auth_room_id, event_id, value)| async move {
				if self.services.pdu_metadata.is_event_rejected(&event_id).await {
					// Rejection will be re-evaluated later
					trace!(%event_id, "Un-rejecting event");
					self.services.pdu_metadata.unmark_event_rejected(&event_id);
				}
				trace!(%event_id, "Validating event");
				if auth_room_id != room_id {
					warn!(%event_id, %room_id, %auth_room_id, "PDU in send_join auth chain belongs to a different room (dropping)");
					return None;
				}

				crate::rooms::event_handler::Service::pdu_format_check_1(&value, room_version_rules, create_event.event_id()).inspect_err(|e| {
					warn!(%event_id, "PDU in send_join auth chain failed format check (dropping): {e:?}");
				}).ok()?;

				let value = self.services.event_handler.signature_hash_check_2_3(value, room_version_rules).await.inspect_err(|e| {
					warn!(%event_id, "PDU in send_join auth chain failed signature check (dropping): {e:?}");
				}).ok()?;

				Some((event_id, value))
			})
			.collect::<HashMap<_, _>>()
			.await;
		debug!(
			elapsed=?auth_chain_timing_start.elapsed(),
			"Finished validating auth chain ({}/{} events passed validation)",
			unauthed_auth_chain.len(),
			response.room_state.auth_chain.len()
		);
		let auth_chain = build_local_dag(&unauthed_auth_chain, DagBuilderTree::AuthEvents)
			.await?
			.into_iter()
			.stream()
			.wide_filter_map(async |event_id| {
				// Perform PDU check 4 to make sure the event doesn't need
				// rejecting
				let pdu =
					PduEvent::from_id_val(&event_id, unauthed_auth_chain[&event_id].clone())
						.expect("We already validated auth chian PDUs");
				let auth_state = pdu
					.auth_events()
					.filter_map(|event_id| {
						PduEvent::from_id_val(
							event_id,
							unauthed_auth_chain.get(event_id)?.clone(),
						)
						.map(|pdu| {
							Some((
								pdu.kind().with_state_key(
									pdu.state_key()
										.expect("Auth chain events must have a state key"),
								),
								pdu,
							))
						})
						.ok()?
					})
					.collect::<HashMap<_, _>>();
				self.services
					.event_handler
					.auth_state_check_4(
						&pdu,
						room_version_rules,
						create_event.as_pdu(),
						&auth_state,
					)
					.await
					.inspect(|result| {
						if !*result {
							// NOTE: usually we don't *drop* events that fail PDU check 4, but
							// send_join handling is special. Instead of rejecting this event and
							// consequently every other event that depends on it, we simply drop
							// it from the chain
							warn!(%event_id, "Auth chain event failed self-authorization (dropping)");
						}
					})
					.inspect_err(
						|e| error!(%event_id, ?e, "Failed to run auth check on auth chain event"),
					)
					.unwrap_or_default()
					.then_some((event_id, pdu))
			})
			.collect::<BTreeMap<_, _>>()
			.await;
		debug!(
			elapsed=?auth_chain_timing_start.elapsed(),
			"Finished authentication auth chain ({}/{} events passed authentication)",
			auth_chain.len(),
			unauthed_auth_chain.len()
		);
		// Now we need to persist them
		auth_chain
			.keys()
			.stream()
			.for_each_concurrent(automatic_width(), async |event_id| {
				if self.services.timeline.get_pdu(event_id).await.is_err() {
					// Only add events that we don't already have
					self.services.outlier.add_pdu_outlier(
						event_id,
						unauthed_auth_chain
							.get(event_id)
							.expect("authorized auth chain event must be in auth chain map"),
					);
				}
			})
			.await;
		info!(elapsed=?auth_chain_timing_start.elapsed(), "Finished processing authentication events");

		Ok(auth_chain)
	}

	async fn process_send_join_state(
		&self,
		untrusted_state_before: HashMap<OwnedEventId, &CanonicalJsonObject>,
		auth_chain: &BTreeMap<OwnedEventId, PduEvent>,
		room_version_rules: &RoomVersionRules,
		create_event: &PduEvent,
	) -> Result<HashMap<StateTypeKey, PduEvent>> {
		let state_timing_start = Instant::now();
		let state_size = untrusted_state_before.len();
		// NOTE: unlike the auth chain, we need to run PDU checks 1 through 4.
		let state_before = untrusted_state_before
			.into_iter()
			.stream()
			.broad_filter_map(|(event_id, value)| async move {
				crate::rooms::event_handler::Service::pdu_format_check_1(
					value,
					room_version_rules,
					create_event.event_id(),
				)
				.inspect_err(|e| {
					warn!(%event_id, "PDU in send_join state failed format check (dropping): {e:?}");
				})
				.ok()?;

				let value = self
					.services
					.event_handler
					.signature_hash_check_2_3(value.clone(), room_version_rules)
					.await
					.inspect_err(|e| {
						warn!(%event_id, "PDU in send_join state failed signature check (dropping): {e:?}");
					})
					.ok()?;

				let pdu = PduEvent::from_id_val(&event_id, value.clone())
					.expect("We already validated this state event");
				let auth_state = pdu
					.auth_events()
					.filter_map(|event_id| {
						auth_chain.get(event_id).map(|p| {
							(
								p.kind().with_state_key(
									p.state_key()
										.expect("auth chain events must have a state key"),
								),
								p.clone(),
							)
						})
					})
					.collect::<HashMap<_, _>>();
				self.services
					.event_handler
					.auth_state_check_4(
						&pdu,
						room_version_rules,
						create_event.as_pdu(),
						&auth_state,
					)
					.await
					.inspect(|result| {
						if !*result {
							warn!(%event_id, "State event failed self-authorization (dropping)");
						}
					})
					.inspect_err(
						|e| error!(%event_id, ?e, "Failed to run auth check on state event"),
					)
					.unwrap_or_default()
					.then_some((pdu, value))
			})
			.collect::<Vec<_>>()
			.await;
		debug!(
			elapsed=?state_timing_start.elapsed(),
			"Finished validation and authentication of room state ({}/{} events retained)",
			state_before.len(),
			state_size,
		);

		state_before
			.iter()
			.stream()
			.for_each_concurrent(automatic_width(), async |(pdu, pdu_json)| {
				if self
					.services
					.timeline
					.get_pdu(pdu.event_id())
					.await
					.is_err()
				{
					// Only add events that we don't already have
					self.services
						.outlier
						.add_pdu_outlier(pdu.event_id(), pdu_json);
				}
			})
			.await;

		info!(
			elapsed=?state_timing_start.elapsed(),
			"Finished processing send join state ({}/{} events)",
			state_before.len(),
			state_size,
		);

		let state_before_by_key = state_before
			.into_iter()
			.map(|(pdu, _)| {
				(
					pdu.kind().with_state_key(
						pdu.state_key().expect("state event must have a state key"),
					),
					pdu,
				)
			})
			.collect::<HashMap<_, _>>();

		Ok(state_before_by_key)
	}

	async fn process_send_join_membership_event(
		&self,
		membership_event: &serde_json::value::RawValue,
		event_id: &EventId,
		state_before: &HashMap<StateTypeKey, PduEvent>,
		state_before_by_id: &HashMap<OwnedEventId, &PduEvent>,
		room_version_rules: &RoomVersionRules,
	) -> Result<(PduEvent, CanonicalJsonObject)> {
		let (_, calculated_event_id, value) = self
			.services
			.event_handler
			.parse_incoming_pdu(membership_event, Some(room_version_rules))
			.await?;

		if calculated_event_id != event_id {
			return Err!(Request(InvalidParam(debug_warn!(
				expected=%event_id,
				actual=%calculated_event_id,
				"Remote server returned a different membership event to the one we sent it"
			))));
		}

		if self.services.pdu_metadata.is_event_rejected(event_id).await {
			// Rejection will be re-evaluated later
			trace!(%event_id, "Un-rejecting event");
			self.services.pdu_metadata.unmark_event_rejected(event_id);
		}

		let create_event = state_before
			.get(&StateEventType::RoomCreate.with_state_key(""))
			.expect("create event must be in state before");
		crate::rooms::event_handler::Service::pdu_format_check_1(
			&value,
			room_version_rules,
			create_event.event_id(),
		)?;

		let value = self
			.services
			.event_handler
			.signature_hash_check_2_3(value, room_version_rules)
			.await?;

		let pdu = PduEvent::from_id_val(event_id, value.clone())
			.expect("We already validated this state event");

		// Perform check 4 to make sure the PDU is self-authorised
		let auth_state = pdu
			.auth_events()
			.filter_map(|event_id| {
				state_before_by_id
					.get(event_id)
					.copied()
					.map(|p| (p.kind().with_state_key(p.state_key().unwrap()), p.clone()))
			})
			.collect::<HashMap<_, _>>();
		if !self
			.services
			.event_handler
			.auth_state_check_4(&pdu, room_version_rules, create_event.as_pdu(), &auth_state)
			.await?
		{
			return Err!(Request(Forbidden("Membership event was not self-authorised")));
		}

		// We can also perform check 5 (state before), since the state returned in the
		// response is the state just before our join. However, state_before_check_5
		// (part of the event_handler service) won't understand this, so we have to do
		// the check manually.
		let state_fetch =
			|k: StateEventType, s: StateKey| ready(state_before.get(&(k, s)).cloned());

		// PDU check: 5
		let auth_check = state_res::event_auth::auth_check(
			room_version_rules,
			&pdu,
			None, // TODO: third party invite
			|ty, sk| state_fetch(ty.clone(), sk.into()),
			create_event.as_pdu(),
		)
		.await
		.map_err(|e| err!(Request(Forbidden("Auth check failed: {e:?}"))))?;
		if !auth_check {
			return Err!(Request(Forbidden(
				"Membership event was not authorised based on the state before it"
			)));
		}

		Ok((pdu, value))
	}
}
