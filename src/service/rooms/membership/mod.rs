use std::{collections::HashMap, sync::Arc};

use conduwuit::{
	Err, Pdu, Result, Server, debug, debug_info, debug_warn, err, error, info, is_true,
	matrix::{
		StateKey,
		event::{gen_event_id, gen_event_id_canonical_json},
	},
	pdu::PartialPdu,
	state_res, trace,
	utils::{self, IterStream, ReadyExt, to_canonical_object},
	warn,
};
use database::Database;
use futures::{FutureExt, StreamExt, TryFutureExt, join};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedRoomId, OwnedServerName, OwnedUserId, RoomId,
	RoomVersionId, UserId,
	api::{
		error::{ErrorKind, IncompatibleRoomVersionErrorData},
		federation,
	},
	canonical_json::to_canonical_value,
	events::{
		StateEventType, StaticEventContent,
		room::{
			join_rules::RoomJoinRulesEventContent,
			member::{MembershipState, RoomMemberEventContent},
		},
	},
	room::{AllowRule, JoinRule},
};

use crate::{
	Dep, antispam, globals,
	rooms::{
		metadata, outlier, short,
		state::{self, RoomMutexGuard},
		state_accessor, state_cache,
		state_compressor::{self, CompressedState, HashSetCompressStateEvent},
		timeline::{self, pdu_fits},
	},
	sending, server_keys, users,
};

pub struct Service {
	services: Services,
}

struct Services {
	server: Arc<Server>,
	db: Arc<Database>,
	antispam: Dep<antispam::Service>,
	globals: Dep<globals::Service>,
	metadata: Dep<metadata::Service>,
	outlier: Dep<outlier::Service>,
	sending: Dep<sending::Service>,
	server_keys: Dep<server_keys::Service>,
	short: Dep<short::Service>,
	state: Dep<state::Service>,
	state_accessor: Dep<state_accessor::Service>,
	state_cache: Dep<state_cache::Service>,
	state_compressor: Dep<state_compressor::Service>,
	timeline: Dep<timeline::Service>,
	users: Dep<users::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				server: args.server.clone(),
				db: args.db.clone(),
				antispam: args.depend::<antispam::Service>("antispam"),
				globals: args.depend::<globals::Service>("globals"),
				metadata: args.depend::<metadata::Service>("metadata"),
				outlier: args.depend::<outlier::Service>("rooms::outlier"),
				sending: args.depend::<sending::Service>("sending"),
				server_keys: args.depend::<server_keys::Service>("server_keys"),
				short: args.depend::<short::Service>("rooms::short"),
				state: args.depend::<state::Service>("rooms::state"),
				state_accessor: args.depend::<state_accessor::Service>("rooms::state_accessor"),
				state_cache: args.depend::<state_cache::Service>("rooms::state_cache"),
				state_compressor: args
					.depend::<state_compressor::Service>("rooms::state_compressor"),
				timeline: args.depend::<timeline::Service>("rooms::timeline"),
				users: args.depend::<users::Service>("users"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Join a local user to a room.
	pub async fn join_room(
		&self,
		sender_user: &UserId,
		room_id: &RoomId,
		reason: Option<String>,
		servers: &[OwnedServerName],
	) -> Result<OwnedRoomId> {
		assert!(self.services.globals.user_is_local(sender_user), "user should be local");

		let state_lock = self.services.state.mutex.lock(room_id).await;

		if self
			.services
			.state_cache
			.is_joined(sender_user, room_id)
			.await
		{
			debug_warn!("{sender_user} is already joined in {room_id}");
			return Ok(room_id.to_owned());
		}

		if let Err(e) = self
			.services
			.antispam
			.user_may_join_room(
				sender_user.to_owned(),
				room_id.to_owned(),
				self.services
					.state_cache
					.is_invited(sender_user, room_id)
					.await,
			)
			.await
		{
			warn!("Antispam prevented user {} from joining room {}: {}", sender_user, room_id, e);
			return Err!(Request(Forbidden("You are not allowed to join this room.")));
		}

		let server_in_room = self
			.services
			.state_cache
			.server_in_room(self.services.globals.server_name(), room_id)
			.await;

		// Only check our known membership if we're already in the room.
		// See: https://forgejo.ellis.link/continuwuation/continuwuity/issues/855
		let membership = if server_in_room {
			self.services
				.state_accessor
				.get_member(room_id, sender_user)
				.await
		} else {
			debug!("Ignoring local state for join {room_id}, we aren't in the room yet.");
			Ok(RoomMemberEventContent::new(MembershipState::Leave))
		};

		if let Ok(m) = membership {
			if m.membership == MembershipState::Ban {
				debug_warn!("{sender_user} is banned from {room_id} but attempted to join");
				// TODO: return reason
				return Err!(Request(Forbidden("You are banned from the room.")));
			}
		}

		if !server_in_room && servers.is_empty() {
			return Err!(Request(NotFound(
				"No servers were provided to assist in joining the room remotely, and we are \
				 not already participating in the room."
			)));
		}

		if self.services.antispam.check_all_joins() {
			if let Err(e) = self
				.services
				.antispam
				.meowlnir_accept_make_join(room_id.to_owned(), sender_user.to_owned())
				.await
			{
				warn!(
					"Antispam prevented user {} from joining room {}: {}",
					sender_user, room_id, e
				);
				return Err!(Request(Forbidden("Antispam rejected join request.")));
			}
		}

		if server_in_room {
			self.join_local_room(sender_user, room_id, reason, servers, state_lock)
				.boxed()
				.await?;
		} else {
			// Ask a remote server if we are not participating in this room
			self.join_remote_room(sender_user, room_id, reason, servers, state_lock)
				.boxed()
				.await?;
		}

		Ok(room_id.to_owned())
	}

	#[tracing::instrument(skip_all, fields(%sender_user, %room_id), name = "join_local", level = "info")]
	async fn join_local_room(
		&self,
		sender_user: &UserId,
		room_id: &RoomId,
		reason: Option<String>,
		servers: &[OwnedServerName],
		state_lock: RoomMutexGuard,
	) -> Result {
		info!("Joining room locally");

		let (room_version, join_rules, is_invited) = join!(
			self.services.state.get_room_version(room_id),
			self.services.state_accessor.get_join_rules(room_id),
			self.services.state_cache.is_invited(sender_user, room_id)
		);

		let room_version = room_version?;
		let room_version_rules = room_version.rules().unwrap();

		let mut auth_user: Option<OwnedUserId> = None;
		if !is_invited
			&& matches!(join_rules, JoinRule::Restricted(_) | JoinRule::KnockRestricted(_))
		{
			if room_version_rules.authorization.restricted_join_rule {
				// This is a restricted room, check if we can complete the join requirements
				// locally.
				let needs_auth_user = self
					.user_can_perform_restricted_join(sender_user, room_id)
					.await;
				if needs_auth_user.is_ok_and(is_true!()) {
					// If there was an error or the value is false, we'll try joining over
					// federation. Since it's Ok(true), we can authorise this locally.
					// If we can't select a local user, this will remain None, the join will fail,
					// and we'll fall back to federation.
					auth_user = self
						.select_authorising_user(room_id, sender_user, &state_lock)
						.await
						.ok();
				}
			}
		}

		let mut content = RoomMemberEventContent::new(MembershipState::Join);
		content.displayname = self.services.users.displayname(sender_user).await.ok();
		content.avatar_url = self.services.users.avatar_url(sender_user).await.ok();
		content.blurhash = self.services.users.blurhash(sender_user).await.ok();
		content.reason.clone_from(&reason);
		content.join_authorized_via_users_server = auth_user;

		// Try normal join first
		let Err(error) = self
			.services
			.timeline
			.build_and_append_pdu(
				PartialPdu::state(sender_user.to_string(), &content),
				sender_user,
				Some(room_id),
				&state_lock,
			)
			.await
		else {
			info!("Joined room locally");
			return Ok(());
		};

		if servers.is_empty()
			|| servers.len() == 1 && self.services.globals.server_is_ours(&servers[0])
		{
			if !self.services.metadata.exists(room_id).await {
				return Err!(Request(
					Unknown(
						"Room was not found locally and no servers were found to help us \
						 discover it"
					),
					NOT_FOUND
				));
			}

			return Err(error);
		}

		info!(
			?error,
			remote_servers = %servers.len(),
			"Could not join room locally, attempting remote join",
		);
		self.join_remote_room(sender_user, room_id, reason, servers, state_lock)
			.await
	}

	#[tracing::instrument(skip_all, fields(%sender_user, %room_id), name = "join_remote_room", level = "info")]
	async fn join_remote_room(
		&self,
		sender_user: &UserId,
		room_id: &RoomId,
		reason: Option<String>,
		servers: &[OwnedServerName],
		state_lock: RoomMutexGuard,
	) -> Result {
		info!("Joining {room_id} over federation.");

		let (make_join_response, remote_server) = self
			.make_join_request(sender_user, room_id, servers)
			.await?;

		info!("make_join finished");

		let room_version = make_join_response.room_version.unwrap_or(RoomVersionId::V1);
		let room_version_rules = room_version
			.rules()
			.expect("room version should have defined rules");

		if !self.services.server.supported_room_version(&room_version) {
			// How did we get here?
			return Err!(BadServerResponse(
				"Remote room version {room_version} is not supported"
			));
		}

		let mut join_event_stub: CanonicalJsonObject =
			serde_json::from_str(make_join_response.event.get()).map_err(|e| {
				err!(BadServerResponse(warn!(
					"Invalid make_join event json received from server: {e:?}"
				)))
			})?;

		let join_authorized_via_users_server = {
			if room_version_rules
				.signatures
				.check_join_authorised_via_users_server
			{
				join_event_stub
					.get("content")
					.map(|s| {
						s.as_object()?
							.get("join_authorised_via_users_server")?
							.as_str()
					})
					.and_then(|s| OwnedUserId::try_from(s.unwrap_or_default()).ok())
			} else {
				None
			}
		};

		join_event_stub.insert(
			"origin_server_ts".to_owned(),
			CanonicalJsonValue::Integer(
				utils::millis_since_unix_epoch()
					.try_into()
					.expect("Timestamp is valid js_int value"),
			),
		);

		let mut join_content = RoomMemberEventContent::new(MembershipState::Join);
		join_content.displayname = self.services.users.displayname(sender_user).await.ok();
		join_content.avatar_url = self.services.users.avatar_url(sender_user).await.ok();
		join_content.blurhash = self.services.users.blurhash(sender_user).await.ok();
		join_content.reason = reason;
		join_content
			.join_authorized_via_users_server
			.clone_from(&join_authorized_via_users_server);

		join_event_stub.insert(
			"content".to_owned(),
			to_canonical_value(join_content).expect("event is valid, we just created it"),
		);

		// Remove event id if it exists
		join_event_stub.remove("event_id");

		// In order to create a compatible ref hash (EventID) the `hashes` field needs
		// to be present
		self.services
			.server_keys
			.hash_and_sign_event(&mut join_event_stub, &room_version_rules)?;

		// Generate event id
		let event_id = gen_event_id(&join_event_stub, &room_version_rules)?;

		// Add event_id back
		join_event_stub
			.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.clone().into()));

		// It has enough fields to be called a proper event now
		let mut join_event = join_event_stub;

		info!("Asking {remote_server} for send_join in room {room_id}");
		let send_join_request = federation::membership::create_join_event::v2::Request::new(
			room_id.to_owned(),
			event_id.clone(),
			self.services
				.sending
				.convert_to_outgoing_federation_event(join_event.clone())
				.await,
		);

		let send_join_response = match self
			.services
			.sending
			.send_synapse_request(&remote_server, send_join_request)
			.await
		{
			| Ok(response) => response,
			| Err(e) => {
				error!("send_join failed: {e}");
				return Err(e);
			},
		};

		info!("send_join finished");

		if join_authorized_via_users_server.is_some() {
			if let Some(signed_raw) = &send_join_response.room_state.event {
				debug_info!(
					"There is a signed event with join_authorized_via_users_server. This room \
					 is probably using restricted joins. Adding signature to our event"
				);

				let (signed_event_id, signed_value) =
					gen_event_id_canonical_json(signed_raw, &room_version_rules).map_err(
						|e| {
							err!(Request(BadJson(warn!(
								"Could not convert event to canonical JSON: {e}"
							))))
						},
					)?;

				if signed_event_id != event_id {
					return Err!(Request(BadJson(warn!(
						%signed_event_id, %event_id,
						"Server {remote_server} sent event with wrong event ID"
					))));
				}

				match signed_value["signatures"]
					.as_object()
					.ok_or_else(|| {
						err!(BadServerResponse(warn!(
							"Server {remote_server} sent invalid signatures type"
						)))
					})
					.and_then(|e| {
						e.get(remote_server.as_str()).ok_or_else(|| {
							err!(BadServerResponse(warn!(
								"Server {remote_server} did not send its signature for a \
								 restricted room"
							)))
						})
					}) {
					| Ok(signature) => {
						join_event
							.get_mut("signatures")
							.expect("we created a valid pdu")
							.as_object_mut()
							.expect("we created a valid pdu")
							.insert(remote_server.to_string(), signature.clone());
					},
					| Err(e) => {
						warn!(
							"Server {remote_server} sent invalid signature in send_join \
							 signatures for event {signed_value:?}: {e:?}",
						);
					},
				}
			}
		}

		self.services.short.get_or_create_shortroomid(room_id).await;

		info!("Parsing join event");
		let parsed_join_pdu = Pdu::from_id_val(&event_id, join_event.clone())
			.map_err(|e| err!(BadServerResponse("Invalid join event PDU: {e:?}")))?;

		info!("Acquiring server signing keys for response events");
		let resp_events = &send_join_response.room_state;
		let resp_state = &resp_events.state;
		let resp_auth = &resp_events.auth_chain;
		self.services
			.server_keys
			.acquire_events_pubkeys(resp_auth.iter().chain(resp_state.iter()))
			.await;

		info!("Going through send_join response room_state");
		let cork = self.services.db.cork_and_flush();
		let state = send_join_response
			.room_state
			.state
			.iter()
			.stream()
			.then(|pdu| {
				self.services
					.server_keys
					.validate_and_add_event_id_no_fetch(pdu, &room_version_rules)
					.inspect_err(|e| {
						debug_warn!(
							"Could not validate send_join response room_state event: {e:?}"
						);
					})
					.inspect(|_| {
						debug!("Completed validating send_join response room_state event");
					})
			})
			.ready_filter_map(Result::ok)
			.fold(HashMap::new(), |mut state, (event_id, value)| async move {
				let pdu = match Pdu::from_id_val(&event_id, value.clone()) {
					| Ok(pdu) => pdu,
					| Err(e) => {
						debug_warn!("Invalid PDU in send_join response: {e:?}: {value:#?}");
						return state;
					},
				};
				if !pdu_fits(&mut value.clone()) {
					warn!(
						"dropping incoming PDU {event_id} in room {room_id} from room join \
						 because it exceeds 65535 bytes or is otherwise too large."
					);
					return state;
				}
				self.services.outlier.add_pdu_outlier(&event_id, &value);
				if let Some(state_key) = &pdu.state_key {
					let shortstatekey = self
						.services
						.short
						.get_or_create_shortstatekey(&pdu.kind.to_string().into(), state_key)
						.await;

					state.insert(shortstatekey, pdu.event_id.clone());
				}
				state
			})
			.await;

		drop(cork);

		info!("Going through send_join response auth_chain");
		let cork = self.services.db.cork_and_flush();
		send_join_response
			.room_state
			.auth_chain
			.iter()
			.stream()
			.then(|pdu| {
				self.services
					.server_keys
					.validate_and_add_event_id_no_fetch(pdu, &room_version_rules)
			})
			.ready_filter_map(Result::ok)
			.ready_for_each(|(event_id, value)| {
				trace!(%event_id, "Adding PDU as an outlier from send_join auth_chain");
				self.services.outlier.add_pdu_outlier(&event_id, &value);
			})
			.await;

		drop(cork);

		debug!("Running send_join auth check");
		let fetch_state = &state;
		let state_fetch = |k: StateEventType, s: StateKey| async move {
			let shortstatekey = self.services.short.get_shortstatekey(&k, &s).await.ok()?;

			let event_id = fetch_state.get(&shortstatekey)?;
			self.services.timeline.get_pdu(event_id).await.ok()
		};

		let auth_check = state_res::event_auth::auth_check(
			&room_version.rules().unwrap(),
			&parsed_join_pdu,
			None, // TODO: third party invite
			|k, s| state_fetch(k.clone(), s.into()),
			&state_fetch(StateEventType::RoomCreate, "".into())
				.await
				.expect("create event is missing from send_join auth"),
		)
		.await
		.map_err(|e| err!(Request(Forbidden(warn!("Auth check failed: {e:?}")))))?;

		if !auth_check {
			return Err!(Request(Forbidden("Auth check failed")));
		}

		info!("Compressing state from send_join");
		let compressed: CompressedState = self
			.services
			.state_compressor
			.compress_state_events(state.iter().map(|(ssk, eid)| (ssk, eid.as_ref())))
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
			.save_state(room_id, Arc::new(compressed))
			.await?;

		debug!("Forcing state for new room");
		self.services
			.state
			.force_state(room_id, statehash_before_join, added, removed, &state_lock)
			.await?;

		debug!("Updating joined counts for new room");
		self.services.state_cache.update_joined_count(room_id).await;

		// We append to state before appending the pdu, so we don't have a moment in
		// time with the pdu without it's state. This is okay because append_pdu can't
		// fail.
		let statehash_after_join = self
			.services
			.state
			.append_to_state(&parsed_join_pdu, room_id)
			.await?;

		info!("Appending new room join event");
		self.services
			.timeline
			.append_pdu(
				&parsed_join_pdu,
				join_event,
				std::iter::once(parsed_join_pdu.event_id.as_ref()),
				&state_lock,
				room_id,
			)
			.await?;

		info!("Setting final room state for new room");
		// We set the room state after inserting the pdu, so that we never have a moment
		// in time where events in the current room state do not exist
		self.services
			.state
			.set_room_state(room_id, statehash_after_join, &state_lock);

		Ok(())
	}

	async fn make_join_request(
		&self,
		sender_user: &UserId,
		room_id: &RoomId,
		servers: &[OwnedServerName],
	) -> Result<(federation::membership::prepare_join_event::v1::Response, OwnedServerName)> {
		let mut make_join_counter: usize = 1;

		for remote_server in servers {
			if self.services.globals.server_is_ours(remote_server) {
				continue;
			}
			info!(
				"Asking {remote_server} for make_join (attempt {make_join_counter}/{})",
				servers.len()
			);

			let mut request = federation::membership::prepare_join_event::v1::Request::new(
				room_id.to_owned(),
				sender_user.to_owned(),
			);
			request.ver = self.services.server.supported_room_versions().collect();

			let make_join_response = self
				.services
				.sending
				.send_federation_request(remote_server, request)
				.await;

			trace!("make_join response: {:?}", make_join_response);
			make_join_counter = make_join_counter.saturating_add(1);

			match make_join_response {
				| Ok(response) => {
					info!("Received make_join response from {remote_server}");
					if let Err(e) = validate_remote_member_event_stub(
						&MembershipState::Join,
						sender_user,
						room_id,
						&to_canonical_object(&response.event)?,
					) {
						warn!("make_join response from {remote_server} failed validation: {e}");
						continue;
					}
					return Ok((response, remote_server.clone()));
				},
				| Err(e) => match e.kind() {
					| ErrorKind::UnableToAuthorizeJoin => {
						info!(
							"{remote_server} was unable to verify the joining user satisfied \
							 restricted join requirements: {e}. Will continue trying."
						);
					},
					| ErrorKind::UnableToGrantJoin => {
						info!(
							"{remote_server} believes the joining user satisfies restricted \
							 join rules, but is unable to authorise a join for us. Will \
							 continue trying."
						);
					},
					| ErrorKind::IncompatibleRoomVersion(IncompatibleRoomVersionErrorData {
						room_version,
						..
					}) => {
						warn!(
							"{remote_server} reports the room we are trying to join is \
							 v{room_version}, which we do not support."
						);
						return Err(e);
					},
					| ErrorKind::Forbidden => {
						warn!("{remote_server} refuses to let us join: {e}.");
						return Err(e);
					},
					| ErrorKind::NotFound => {
						info!(
							"{remote_server} does not know about {room_id}: {e}. Will continue \
							 trying."
						);
					},
					| _ => {
						info!("{remote_server} failed to make_join: {e}. Will continue trying.");
					},
				},
			}
		}
		info!("All {} servers were unable to assist in joining {room_id} :(", servers.len());
		Err!(BadServerResponse("No server available to assist in joining."))
	}

	/// Attempts to find a user who is able to issue an invite in the target
	/// room.
	pub async fn select_authorising_user<'a>(
		&self,
		room_id: &'a RoomId,
		user_id: &'a UserId,
		state_lock: &'a RoomMutexGuard,
	) -> Result<OwnedUserId> {
		let candidates = self.services.state_cache.local_users_in_room(room_id);

		let mut candidates = std::pin::pin!(candidates);

		while let Some(candidate) = candidates.next().await {
			if self
				.services
				.state_accessor
				.user_can_invite(room_id, &candidate, user_id, state_lock)
				.await
			{
				return Ok(candidate);
			}
		}

		Err!(Request(UnableToGrantJoin(
			"No user on this server is able to assist in joining."
		)))
	}

	/// Checks whether the given user can join the given room via a restricted
	/// join.
	pub(crate) async fn user_can_perform_restricted_join(
		&self,
		user_id: &UserId,
		room_id: &RoomId,
	) -> Result<bool> {
		let Ok(join_rules_event_content) = self
			.services
			.state_accessor
			.room_state_get_content::<RoomJoinRulesEventContent>(
				room_id,
				&StateEventType::RoomJoinRules,
				"",
			)
			.await
		else {
			// No join rules means there's nothing to authorise (defaults to invite)
			return Ok(false);
		};

		let (JoinRule::Restricted(r) | JoinRule::KnockRestricted(r)) =
			join_rules_event_content.join_rule
		else {
			// This is not a restricted room
			return Ok(false);
		};

		if r.allow.is_empty() {
			// This will never be authorisable, return forbidden.
			return Err!(Request(Forbidden("You are not invited to this room.")));
		}

		let mut could_satisfy = true;
		for allow_rule in &r.allow {
			match allow_rule {
				| AllowRule::RoomMembership(membership) => {
					if !self
						.services
						.state_cache
						.server_in_room(self.services.globals.server_name(), &membership.room_id)
						.await
					{
						// Since we can't check this room, mark could_satisfy as false
						// so that we can return M_UNABLE_TO_AUTHORIZE_JOIN later.
						could_satisfy = false;
						continue;
					}

					if self
						.services
						.state_cache
						.is_joined(user_id, &membership.room_id)
						.await
					{
						debug!(
							"User {} is allowed to join room {} via membership in room {}",
							user_id, room_id, membership.room_id
						);
						return Ok(true);
					}
				},
				| other if other.rule_type() == "fi.mau.spam_checker" =>
					return match self
						.services
						.antispam
						.meowlnir_accept_make_join(room_id.to_owned(), user_id.to_owned())
						.await
					{
						| Ok(()) => Ok(true),
						| Err(_) => Err!(Request(Forbidden("Antispam rejected join request."))),
					},
				| _ => {
					// We don't recognise this join rule, so we cannot satisfy the request.
					could_satisfy = false;
					debug_info!(
						"Unsupported allow rule in restricted join for room {}: {:?}",
						room_id,
						allow_rule
					);
				},
			}
		}

		if could_satisfy {
			// We were able to check all the restrictions and can be certain that the
			// prospective member is not permitted to join.
			Err!(Request(Forbidden(
				"You do not belong to any of the rooms or spaces required to join this room."
			)))
		} else {
			// We were unable to check all the restrictions. This usually means we aren't in
			// one of the rooms this one is restricted to, ergo can't check its state for
			// the user's membership, and consequently the user *might* be able to join if
			// they ask another server.
			Err!(Request(UnableToAuthorizeJoin(
				"You do not belong to any of the recognised rooms or spaces required to join \
				 this room, but this server is unable to verify every requirement. You may be \
				 able to join via another server."
			)))
		}
	}
}

/// Validates that an event returned from a remote server by `/make_*`
/// actually is a membership event with the expected fields.
///
/// Without checking this, the remote server could use the remote membership
/// mechanism to trick our server into signing arbitrary malicious events.
pub fn validate_remote_member_event_stub(
	membership: &MembershipState,
	user_id: &UserId,
	room_id: &RoomId,
	event_stub: &CanonicalJsonObject,
) -> Result<()> {
	let Some(event_type) = event_stub.get("type") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing type field"
		));
	};
	if event_type != &RoomMemberEventContent::TYPE {
		return Err!(BadServerResponse(
			"Remote server returned member event with invalid event type"
		));
	}

	let Some(sender) = event_stub.get("sender") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing sender field"
		));
	};
	if sender != &user_id.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect sender"
		));
	}

	let Some(state_key) = event_stub.get("state_key") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing state_key field"
		));
	};
	if state_key != &user_id.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect state_key"
		));
	}

	let Some(event_room_id) = event_stub.get("room_id") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing room_id field"
		));
	};
	if event_room_id != &room_id.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect room_id"
		));
	}

	let Some(content) = event_stub
		.get("content")
		.and_then(|content| content.as_object())
	else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing content field"
		));
	};
	let Some(event_membership) = content.get("membership") else {
		return Err!(BadServerResponse(
			"Remote server returned member event with missing membership field"
		));
	};
	if event_membership != &membership.as_str() {
		return Err!(BadServerResponse(
			"Remote server returned member event with incorrect membership type"
		));
	}

	Ok(())
}
