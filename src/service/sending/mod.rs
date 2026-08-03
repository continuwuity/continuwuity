pub mod antispam;
mod appservice;
mod data;
mod dest;
mod sender;

use std::{
	collections::HashMap,
	fmt::Debug,
	hash::{DefaultHasher, Hash, Hasher},
	iter::once,
	sync::Arc,
	time::SystemTime,
};

use async_trait::async_trait;
use conduwuit::{
	Result, Server, SyncRwLock, debug, debug_warn, err, error,
	smallvec::SmallVec,
	utils::{
		ReadyExt, TryFutureExtExt, TryReadyExt, available_parallelism,
		continue_exponential_backoff, math::usize_from_u64_truncated, millis_since_unix_epoch,
		time::exponential_backoff::min_exp_backoff_duration,
	},
	warn,
};
use futures::{FutureExt, Stream, StreamExt};
use ruma::{
	OwnedServerName, RoomId, ServerName, UserId,
	api::{
		OutgoingRequest,
		auth_scheme::{NoAccessToken, NoAuthentication, SendAccessToken},
		federation::authentication::ServerSignatures,
		path_builder::PathBuilder,
	},
};
use tokio::{task, task::JoinSet};

use self::data::Data;
pub use self::{
	dest::Destination,
	sender::{EDU_LIMIT, PDU_LIMIT},
};
use crate::{
	Dep, account_data, client,
	federation::{self, FederationPathBuilderInput},
	globals, presence, pusher,
	rooms::{self, timeline::RawPduId},
	users,
};

pub struct Service {
	pub db: Data,
	server: Arc<Server>,
	services: Services,
	channels: Vec<(loole::Sender<Msg>, loole::Receiver<Msg>)>,
	remote_health: SyncRwLock<HashMap<OwnedServerName, (u32, u64)>>,
}

struct Services {
	client: Dep<client::Service>,
	globals: Dep<globals::Service>,
	state_cache: Dep<rooms::state_cache::Service>,
	user: Dep<rooms::user::Service>,
	users: Dep<users::Service>,
	presence: Dep<presence::Service>,
	read_receipt: Dep<rooms::read_receipt::Service>,
	timeline: Dep<rooms::timeline::Service>,
	account_data: Dep<account_data::Service>,
	appservice: Dep<crate::appservice::Service>,
	pusher: Dep<pusher::Service>,
	federation: Dep<federation::Service>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Msg {
	dest: Destination,
	event: SendingEvent,
	queue_id: Vec<u8>,
}

#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SendingEvent {
	Pdu(RawPduId), // pduid
	Edu(EduBuf),   // edu json
	Flush,         // none
}

pub type EduBuf = SmallVec<[u8; EDU_BUF_CAP]>;
pub type EduVec = SmallVec<[EduBuf; EDU_VEC_CAP]>;

const EDU_BUF_CAP: usize = 128;
const EDU_VEC_CAP: usize = 1;

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let num_senders = num_senders(&args);
		Ok(Arc::new(Self {
			db: Data::new(&args),
			server: args.server.clone(),
			services: Services {
				client: args.depend::<client::Service>("client"),
				globals: args.depend::<globals::Service>("globals"),
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
				user: args.depend::<rooms::user::Service>("rooms::user"),
				users: args.depend::<users::Service>("users"),
				presence: args.depend::<presence::Service>("presence"),
				read_receipt: args.depend::<rooms::read_receipt::Service>("rooms::read_receipt"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
				account_data: args.depend::<account_data::Service>("account_data"),
				appservice: args.depend::<crate::appservice::Service>("appservice"),
				pusher: args.depend::<pusher::Service>("pusher"),
				federation: args.depend::<federation::Service>("federation"),
			},
			channels: (0..num_senders).map(|_| loole::unbounded()).collect(),
			remote_health: SyncRwLock::new(HashMap::new()),
		}))
	}

	async fn worker(self: Arc<Self>) -> Result {
		let mut senders =
			self.channels
				.iter()
				.enumerate()
				.fold(JoinSet::new(), |mut joinset, (id, _)| {
					let self_ = self.clone();
					let worker = self_.sender(id);
					let worker = if self.unconstrained() {
						task::unconstrained(worker).boxed()
					} else {
						worker.boxed()
					};

					let runtime = self.server.runtime();
					let _abort = joinset.spawn_on(worker, runtime);
					joinset
				});

		while let Some(ret) = senders.join_next_with_id().await {
			match ret {
				| Ok((id, _)) => {
					debug!(?id, "sender worker finished");
				},
				| Err(error) => {
					error!(id = ?error.id(), ?error, "sender worker finished");
				},
			}
		}

		Ok(())
	}

	fn interrupt(&self) {
		for (sender, _) in &self.channels {
			if !sender.is_closed() {
				sender.close();
			}
		}
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }

	fn unconstrained(&self) -> bool { true }
}

impl Service {
	#[tracing::instrument(skip(self, pdu_id, user, pushkey), level = "debug")]
	pub fn send_pdu_push(&self, pdu_id: &RawPduId, user: &UserId, pushkey: String) -> Result {
		let dest = Destination::Push(user.to_owned(), pushkey);
		let event = SendingEvent::Pdu(*pdu_id);
		let _cork = self.db.db.cork();
		let keys = self.db.queue_requests(once((&event, &dest)));
		self.dispatch(Msg {
			dest,
			event,
			queue_id: keys.into_iter().next().expect("request queue key"),
		})
	}

	#[tracing::instrument(skip(self), level = "debug")]
	pub fn send_pdu_appservice(&self, appservice_id: String, pdu_id: RawPduId) -> Result {
		let dest = Destination::Appservice(appservice_id);
		let event = SendingEvent::Pdu(pdu_id);
		let _cork = self.db.db.cork();
		let keys = self.db.queue_requests(once((&event, &dest)));
		self.dispatch(Msg {
			dest,
			event,
			queue_id: keys.into_iter().next().expect("request queue key"),
		})
	}

	#[tracing::instrument(skip(self, room_id, pdu_id), level = "debug")]
	pub async fn send_pdu_room(&self, room_id: &RoomId, pdu_id: &RawPduId) -> Result {
		let servers = self
			.services
			.state_cache
			.room_servers(room_id)
			.ready_filter(|server_name| !self.services.globals.server_is_ours(server_name));

		self.send_pdu_servers(servers, pdu_id).await
	}

	#[tracing::instrument(skip(self, servers, pdu_id), level = "debug")]
	pub async fn send_pdu_servers<S>(&self, servers: S, pdu_id: &RawPduId) -> Result
	where
		S: Stream<Item = OwnedServerName> + Send,
	{
		let requests = servers
			.map(|server| (Destination::Federation(server), SendingEvent::Pdu(pdu_id.to_owned())))
			.collect::<Vec<_>>()
			.await;

		let _cork = self.db.db.cork();
		let keys = self.db.queue_requests(requests.iter().map(|(o, e)| (e, o)));

		for ((dest, event), queue_id) in requests.into_iter().zip(keys) {
			self.dispatch(Msg { dest, event, queue_id })?;
		}

		Ok(())
	}

	#[tracing::instrument(skip(self, server, serialized), level = "debug")]
	pub fn send_edu_server(&self, server: &ServerName, serialized: EduBuf) -> Result {
		let dest = Destination::Federation(server.to_owned());
		let event = SendingEvent::Edu(serialized);
		let _cork = self.db.db.cork();
		let keys = self.db.queue_requests(once((&event, &dest)));
		self.dispatch(Msg {
			dest,
			event,
			queue_id: keys.into_iter().next().expect("request queue key"),
		})
	}

	#[tracing::instrument(skip(self, room_id, serialized), level = "debug")]
	pub async fn send_edu_room(&self, room_id: &RoomId, serialized: EduBuf) -> Result {
		let servers = self
			.services
			.state_cache
			.room_servers(room_id)
			.ready_filter(|server_name| !self.services.globals.server_is_ours(server_name));

		self.send_edu_servers(servers, serialized).await
	}

	#[tracing::instrument(skip(self, servers, serialized), level = "debug")]
	pub async fn send_edu_servers<S>(&self, servers: S, serialized: EduBuf) -> Result
	where
		S: Stream<Item = OwnedServerName> + Send,
	{
		let requests = servers
			.map(|server| {
				(Destination::Federation(server), SendingEvent::Edu(serialized.clone()))
			})
			.collect::<Vec<_>>()
			.await;

		let _cork = self.db.db.cork();
		let keys = self.db.queue_requests(requests.iter().map(|(o, e)| (e, o)));

		for ((dest, event), queue_id) in requests.into_iter().zip(keys) {
			self.dispatch(Msg { dest, event, queue_id })?;
		}

		Ok(())
	}

	#[tracing::instrument(skip(self, room_id), level = "debug")]
	pub async fn flush_room(&self, room_id: &RoomId) -> Result<()> {
		let servers = self
			.services
			.state_cache
			.room_servers(room_id)
			.ready_filter(|server_name| !self.services.globals.server_is_ours(server_name));

		self.flush_servers(servers).await
	}

	#[tracing::instrument(skip(self, servers), level = "debug")]
	pub async fn flush_servers<S>(&self, servers: S) -> Result<()>
	where
		S: Stream<Item = OwnedServerName> + Send,
	{
		servers
			.map(Destination::Federation)
			.map(Ok)
			.ready_try_for_each(|dest| {
				self.dispatch(Msg {
					dest,
					event: SendingEvent::Flush,
					queue_id: Vec::<u8>::new(),
				})
			})
			.await
	}

	/// Sends a request to a federation server
	#[inline]
	pub async fn send_federation_request<'i, T>(
		&self,
		dest: &ServerName,
		request: T,
	) -> Result<T::IncomingResponse>
	where
		T: OutgoingRequest<
				Authentication = ServerSignatures,
				PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
			> + Debug
			+ Send,
	{
		self.services.federation.execute(dest, request).await
	}

	/// Like send_federation_request() but with a very large timeout
	#[inline]
	pub async fn send_slow_federation_request<'i, T>(
		&self,
		dest: &ServerName,
		request: T,
	) -> Result<T::IncomingResponse>
	where
		T: OutgoingRequest<
				Authentication = ServerSignatures,
				PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
			> + Debug
			+ Send,
	{
		self.services.federation.execute_slow(dest, request).await
	}

	/// Send an unauthenticated federation request with no X-Matrix header.
	#[inline]
	pub async fn send_unauthenticated_request<'i, T>(
		&self,
		dest: &ServerName,
		request: T,
	) -> Result<T::IncomingResponse>
	where
		T: OutgoingRequest<
				Authentication = NoAuthentication,
				PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
			> + Debug
			+ Send,
	{
		self.services
			.federation
			.execute_unauthenticated(dest, request)
			.await
	}

	/// Send an unauthenticated federation request with no X-Matrix header.
	#[inline]
	pub async fn send_legacy_media_request<'i, T>(
		&self,
		dest: &ServerName,
		request: T,
	) -> Result<T::IncomingResponse>
	where
		T: OutgoingRequest<
				Authentication = NoAccessToken,
				PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
			> + Debug
			+ Send,
	{
		self.services
			.federation
			.execute_on(&self.services.client.federation, dest, request, SendAccessToken::None)
			.await
	}

	/// Clean up queued sending event data
	///
	/// Used after we remove an appservice registration or a user deletes a push
	/// key
	#[tracing::instrument(skip(self), level = "debug")]
	pub async fn cleanup_events(
		&self,
		appservice_id: Option<&str>,
		user_id: Option<&UserId>,
		push_key: Option<&str>,
	) -> Result {
		match (appservice_id, user_id, push_key) {
			| (None, Some(user_id), Some(push_key)) => {
				self.db
					.delete_all_requests_for(&Destination::Push(
						user_id.to_owned(),
						push_key.to_owned(),
					))
					.await;

				Ok(())
			},
			| (Some(appservice_id), None, None) => {
				self.db
					.delete_all_requests_for(&Destination::Appservice(appservice_id.to_owned()))
					.await;

				Ok(())
			},
			| _ => {
				debug_warn!("cleanup_events called with too many or too few arguments");
				Ok(())
			},
		}
	}

	fn dispatch(&self, msg: Msg) -> Result {
		let shard = self.shard_id(&msg.dest);
		let sender = &self
			.channels
			.get(shard)
			.expect("missing sender worker channels")
			.0;

		debug_assert!(!sender.is_full(), "channel full");
		debug_assert!(!sender.is_closed(), "channel closed");
		sender.send(msg).map_err(|e| err!("{e}"))
	}

	pub(super) fn shard_id(&self, dest: &Destination) -> usize {
		if self.channels.len() <= 1 {
			return 0;
		}

		let mut hash = DefaultHasher::default();
		dest.hash(&mut hash);

		let hash: u64 = hash.finish();
		let hash = usize_from_u64_truncated(hash);

		let chans = self.channels.len().max(1);
		hash.overflowing_rem(chans).0
	}

	/// Checks if a remote is "healthy". "Healthy" is defined by either:
	///
	/// * The remote has not been marked as having a failed request, OR
	/// * The next retry timestamp is in the past
	pub fn is_healthy(&self, server_name: &ServerName) -> bool {
		let map = self.remote_health.read();
		let unix_now = millis_since_unix_epoch();
		if let Some((_, next_retry)) = map.get(server_name) {
			unix_now >= *next_retry
		} else {
			true
		}
	}

	/// Marks or updates a remote's health status as unhealthy. If the remote is
	/// not already marked as unhealthy, a new entry is created. Otherwise, the
	/// retry count is incremented and
	pub fn hit_unhealthy(&self, server_name: OwnedServerName) {
		// TODO(nex): Can multiple concurrent failures cause this health monitor to
		// rapidly max out?
		//
		// consider: a profile query and key claim query both go out at the same time,
		// and both fail. They both then go to hit this (synchronous) function, which
		// means one of them will increment the failure count by 1, and consequently
		// increase the exp backoff. Then the second failure is allowed to call the
		// function, at which point it increments the failure count AGAIN, increasing
		// the backoff again.
		// Technically, these are two distinct failures. However, from a UX perspective,
		// they happened at the same time, so shouldn't incur a double penalty?
		// Perhaps only incrementing the retry counter when the current next_retry is in
		// the past might help.
		//
		// You know it's a banger thought process when the comment is longer than the
		// code itself.
		let unix_now = millis_since_unix_epoch();
		let mut map = self.remote_health.write();
		let (retries, next_retry) = map.entry(server_name).or_default();

		let min = self.server.config.sender_timeout;
		let max = self.server.config.sender_retry_backoff_limit;

		*retries = retries.saturating_add(1);
		*next_retry = unix_now.saturating_add(
			u64::try_from(min_exp_backoff_duration(min, max, *retries).as_millis())
				.expect("backoff milliseconds should not exceed u64::MAX"),
		);
	}

	/// Marks a server as "healthy" by removing it from the health map.
	///
	/// TODO: flush senders too
	pub fn mark_healthy(&self, server_name: &ServerName) {
		// TODO: We need to make sure the sender flush DOESN'T trigger if this is called
		// by the senders themselves.
		let mut map = self.remote_health.write();
		map.remove(server_name);
	}
}

fn num_senders(args: &crate::Args<'_>) -> usize {
	const MIN_SENDERS: usize = 1;
	// Limit the number of senders to the number of workers threads or number of
	// cores, conservatively.
	let mut max_senders = args.server.metrics.num_workers();

	// Work around some platforms not returning the number of cores.
	let num_cores = available_parallelism();
	if num_cores > 0 {
		max_senders = max_senders.min(num_cores);
	}

	let worker_count = args.server.config.sender_workers;
	if worker_count == 0 {
		max_senders
	} else {
		worker_count.clamp(MIN_SENDERS, max_senders)
	}
}
