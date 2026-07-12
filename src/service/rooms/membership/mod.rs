mod event;
mod join;

use std::{collections::VecDeque, sync::Arc};

use conduwuit::{
	Err, Result, Server, debug, debug_info, debug_warn, info, is_true,
	matrix::event::gen_event_id, pdu::PartialPdu, warn,
};
use database::Database;
use futures::{FutureExt, StreamExt, join};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedRoomId, OwnedServerName, OwnedUserId, RoomId,
	RoomVersionId, UserId,
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
		event_handler,
		membership::join::MakeJoinResult,
		metadata, outlier, pdu_metadata, short,
		state::{self, RoomMutexGuard},
		state_accessor, state_cache,
		state_compressor::{self},
		timeline,
	},
	sending, server_keys, sync, users,
};

pub struct Service {
	services: Services,
}

struct Services {
	server: Arc<Server>,
	db: Arc<Database>,
	antispam: Dep<antispam::Service>,
	event_handler: Dep<event_handler::Service>,
	globals: Dep<globals::Service>,
	metadata: Dep<metadata::Service>,
	outlier: Dep<outlier::Service>,
	pdu_metadata: Dep<pdu_metadata::Service>,
	sending: Dep<sending::Service>,
	server_keys: Dep<server_keys::Service>,
	short: Dep<short::Service>,
	state: Dep<state::Service>,
	state_accessor: Dep<state_accessor::Service>,
	state_cache: Dep<state_cache::Service>,
	state_compressor: Dep<state_compressor::Service>,
	sync: Dep<sync::Service>,
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
				event_handler: args.depend::<event_handler::Service>("rooms::event_handler"),
				globals: args.depend::<globals::Service>("globals"),
				metadata: args.depend::<metadata::Service>("rooms::metadata"),
				outlier: args.depend::<outlier::Service>("rooms::outlier"),
				pdu_metadata: args.depend::<pdu_metadata::Service>("rooms::pdu_metadata"),
				sending: args.depend::<sending::Service>("sending"),
				server_keys: args.depend::<server_keys::Service>("server_keys"),
				short: args.depend::<short::Service>("rooms::short"),
				state: args.depend::<state::Service>("rooms::state"),
				state_accessor: args.depend::<state_accessor::Service>("rooms::state_accessor"),
				state_cache: args.depend::<state_cache::Service>("rooms::state_cache"),
				state_compressor: args
					.depend::<state_compressor::Service>("rooms::state_compressor"),
				sync: args.depend::<sync::Service>("sync"),
				timeline: args.depend::<timeline::Service>("rooms::timeline"),
				users: args.depend::<users::Service>("users"),
			},
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Join a local user to a room. If the room is not local, this will attempt
	/// to join via federation. If the room cannot be joined locally, a
	/// federation join may be attempted. Users trying to join a room they're
	/// already joined to will short-circuit.
	pub async fn join_room(
		&self,
		sender_user: &UserId,
		room_id: &RoomId,
		reason: Option<String>,
		servers: Vec<OwnedServerName>,
	) -> Result<OwnedRoomId> {
		assert!(self.services.globals.user_is_local(sender_user), "user should be local");

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
			self.join_local_room(sender_user, room_id, reason, servers)
				.boxed()
				.await?;
		} else {
			// Ask a remote server if we are not participating in this room
			self.join_remote_room(sender_user, room_id, reason, servers)
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
		servers: Vec<OwnedServerName>,
	) -> Result {
		info!("Joining room locally");

		let state_lock = self.services.state.mutex.lock(room_id).await;
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
		drop(state_lock);

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
		Box::pin(self.join_remote_room(sender_user, room_id, reason, servers)).await
	}

	#[tracing::instrument(skip_all, fields(%sender_user, %room_id), name = "join_remote_room", level = "info")]
	pub async fn join_remote_room(
		&self,
		sender_user: &UserId,
		room_id: &RoomId,
		reason: Option<String>,
		vias: Vec<OwnedServerName>,
	) -> Result {
		info!("Joining {room_id} over federation.");

		// Treat the via list as a "priority queue", and each time a make_join fails,
		// move the server to the back of the queue. This way, if a server is down, we
		// will eventually try all servers in the list.
		let mut priority_queue = VecDeque::from(vias.clone());
		let mut template: Option<CanonicalJsonObject> = None;
		let mut room_version: Option<RoomVersionId> = None;
		let via_count = vias.len();

		for via in vias {
			let response = self.make_join(room_id, sender_user, &via).await;
			match response {
				| MakeJoinResult::Success((t, r)) => {
					template = Some(t);
					room_version = Some(r);
					break;
				},
				| MakeJoinResult::Fatal(e) => {
					return Err(e);
				},
				| MakeJoinResult::Retry => {
					// Pop the front to remove this server from the queue, and
					// then push it to the back.
					// We need to do this vec iter + dequeue to avoid infinite
					// looping when we reprioritise.
					let _ = priority_queue.pop_front();
					priority_queue.push_back(via);
				},
			}
		}

		if template.is_none() {
			info!("All {} servers were unable to assist in joining {room_id} :(", via_count);
			return Err!(BadServerResponse("No server available to assist in joining."));
		}
		let room_version =
			room_version.expect("room_version cannot be None while template is Some");

		info!("make_join finished");

		if !self.services.server.supported_room_version(&room_version) {
			// How did we get here?
			return Err!(BadServerResponse(
				"Remote room version {room_version} is not supported"
			));
		}
		let room_version_rules = room_version
			.rules()
			.expect("room version should have defined rules");

		let mut template = self
			.seed_local_membership_pdu(
				room_id,
				sender_user,
				MembershipState::Join,
				reason,
				template.unwrap(),
				&room_version_rules,
			)
			.await?;

		// In order to create a compatible ref hash (EventID) the `hashes` field needs
		// to be present
		self.services
			.server_keys
			.hash_and_sign_event(&mut template, &room_version_rules)?;

		// Generate event id
		let event_id = gen_event_id(&template, &room_version_rules)?;

		// Add event_id back
		template
			.insert("event_id".to_owned(), CanonicalJsonValue::String(event_id.clone().into()));

		// NOTE: send_join can take a long time to respond, but from the point of view
		// of other servers, we may already have finished joining. This means they
		// sometimes end up sending PDUs to us that we aren't yet ready to accept, and
		// consequently drop. Holding the mutex over the room while processing mitigates
		// this.
		let _room_lock = self
			.services
			.event_handler
			.mutex_federation
			.lock(room_id.as_str())
			.await;
		let state_lock = self.services.state.mutex.lock(room_id).await;
		while let Some(via) = priority_queue.pop_front() {
			let cork = self.services.db.cork_and_sync();
			let result = self
				.send_join(
					room_id.to_owned(),
					event_id.clone(),
					&template,
					&via,
					&room_version_rules,
					&state_lock,
				)
				.await?;
			if result.is_none() {
				continue;
			}
			info!("send_join finished");
			drop(cork);
			self.services.sync.wake_all_joined(room_id).await;
			return Ok(());
		}

		info!(
			"Despite producing a membership template, no server was capable of actually \
			 completing the join for us."
		);
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
