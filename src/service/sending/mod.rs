pub mod antispam;
mod appservice;
mod data;
mod dest;
mod sender;
#[cfg(test)]
mod tests;

use std::{
	collections::HashMap,
	fmt::Debug,
	hash::{DefaultHasher, Hash, Hasher},
	iter::once,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::{Duration, Instant},
};

use async_trait::async_trait;
use conduwuit::{
	Result, Server, debug, debug_warn, err, error,
	metrics::KeyValue,
	smallvec::SmallVec,
	utils::{ReadyExt, TryReadyExt, available_parallelism, math::usize_from_u64_truncated},
	warn,
};
use futures::{FutureExt, Stream, StreamExt, stream::FuturesUnordered};
use ruma::{
	OwnedServerName, RoomId, ServerName, UserId,
	api::{
		OutgoingRequest, auth_scheme::NoAuthentication,
		federation::authentication::ServerSignatures, path_builder::PathBuilder,
	},
};
use tokio::{task, task::JoinSet, time::sleep};

use self::data::Data;
pub use self::{
	dest::Destination,
	sender::{EDU_LIMIT, PDU_LIMIT},
};
use crate::{
	Dep, account_data,
	appservice::{NamespaceRegex, RegistrationInfo},
	client,
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
	workers_alive: AtomicUsize,
}

struct Services {
	alias: Dep<rooms::alias::Service>,
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
const WORKER_RESTART_DELAY: Duration = Duration::from_millis(2500);
const WORKER_RESTART_DELAY_MAX: Duration = Duration::from_secs(300);

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		let num_senders = num_senders(&args);
		Ok(Arc::new(Self {
			db: Data::new(&args),
			server: args.server.clone(),
			services: Services {
				alias: args.depend::<rooms::alias::Service>("rooms::alias"),
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
			workers_alive: AtomicUsize::new(0),
		}))
	}

	async fn worker(self: Arc<Self>) -> Result {
		if self.server.config.startup_netburst {
			self.dispatch_queued(None).await;
		}

		let num_senders = self.channels.len();
		let mut senders = JoinSet::new();
		let mut shards = HashMap::new();
		let mut started = vec![Instant::now(); num_senders];
		let mut failures = vec![0_u32; num_senders];
		let mut restarts = FuturesUnordered::new();
		for id in 0..num_senders {
			self.spawn_sender(&mut senders, &mut shards, id, false);
		}

		loop {
			tokio::select! {
				Some(ret) = senders.join_next_with_id() => {
					self.workers_alive.fetch_sub(1, Ordering::Relaxed);
					let (task_id, panicked) = match &ret {
						| Ok((task_id, _)) => (*task_id, false),
						| Err(error) => (error.id(), error.is_panic()),
					};
					let shard = shards.remove(&task_id).expect("sender worker has a shard");

					if panicked {
						self.server
							.metrics
							.instruments
							.sender_panics
							.add(1, &[KeyValue::new("scope", "worker")]);
					}

					if self.channels[shard].1.is_closed() || !self.server.running() {
						debug!(shard, ?ret, "sender worker finished");
						continue;
					}

					// A worker that ran a while before failing starts over from
					// the shortest delay.
					failures[shard] = if started[shard].elapsed() > WORKER_RESTART_DELAY_MAX {
						1
					} else {
						failures[shard].saturating_add(1)
					};
					let delay = restart_delay(failures[shard]);
					error!(shard, ?ret, ?delay, "sender worker stopped, restarting");
					restarts.push(sleep(delay).map(move |()| shard));
				},
				Some(shard) = restarts.next() => {
					if !self.channels[shard].1.is_closed() {
						started[shard] = Instant::now();
						self.spawn_sender(&mut senders, &mut shards, shard, true);
					}
				},
				() = self.server.until_shutdown(), if !restarts.is_empty() => restarts.clear(),
				else => break,
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
	fn spawn_sender(
		self: &Arc<Self>,
		senders: &mut JoinSet<Result>,
		shards: &mut HashMap<task::Id, usize>,
		id: usize,
		restarted: bool,
	) {
		let worker = self.clone().sender(id, restarted);
		let worker = if crate::Service::unconstrained(&**self) {
			task::unconstrained(worker).boxed()
		} else {
			worker.boxed()
		};

		let handle = senders.spawn_on(worker, self.server.runtime());
		shards.insert(handle.id(), id);
		self.workers_alive.fetch_add(1, Ordering::Relaxed);
	}

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

	#[tracing::instrument(skip(self, serialized), level = "debug")]
	pub fn send_edu_appservice(&self, appservice_id: &str, serialized: EduBuf) -> Result {
		let dest = Destination::Appservice(appservice_id.to_owned());
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
	pub async fn send_edu_appservice_room(
		&self,
		room_id: &RoomId,
		serialized: EduBuf,
	) -> Result<()> {
		self.send_edu_appservice_room_filtered(room_id, serialized, |_| true)
			.await
	}

	#[tracing::instrument(skip(self, room_id, serialized, filter), level = "debug")]
	pub async fn send_edu_appservice_room_filtered<F>(
		&self,
		room_id: &RoomId,
		serialized: EduBuf,
		filter: F,
	) -> Result<()>
	where
		F: Fn(&RegistrationInfo) -> bool + Send,
	{
		let appservices: Vec<_> = self
			.services
			.appservice
			.read()
			.values()
			.filter(|appservice| appservice.registration.receive_ephemeral && filter(appservice))
			.cloned()
			.collect();

		for appservice in appservices {
			let matching_aliases = |aliases: NamespaceRegex| {
				self.services
					.alias
					.local_aliases_for_room(room_id)
					.ready_any(move |room_alias| aliases.is_match(room_alias.as_str()))
			};

			if appservice.rooms.is_match(room_id.as_str())
				|| self
					.services
					.state_cache
					.appservice_in_room(room_id, &appservice)
					.await
				|| matching_aliases(appservice.aliases.clone()).await
			{
				_ = self
					.send_edu_appservice(&appservice.registration.id, serialized.clone())
					.inspect_err(|e| {
						warn!(
							"failed to send EDU to appservice {}: {e:?}",
							appservice.registration.id
						);
					});
			}
		}
		Ok(())
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
			},
			| (Some(appservice_id), None, None) => {
				self.db
					.delete_all_requests_for(&Destination::Appservice(appservice_id.to_owned()))
					.await;
			},
			| _ => {
				debug_warn!("cleanup_events called with too many or too few arguments");
			},
		}

		Ok(())
	}

	fn dispatch(&self, msg: Msg) -> Result {
		let shard = self.shard_id(&msg.dest);
		let sender = &self
			.channels
			.get(shard)
			.expect("missing sender worker channels")
			.0;

		let kind = match msg.event {
			| SendingEvent::Pdu(_) => Some("pdu"),
			| SendingEvent::Edu(_) => Some("edu"),
			| SendingEvent::Flush => None,
		};
		if let Some(kind) = kind {
			self.server
				.metrics
				.instruments
				.sender_events_queued
				.add(1, &[
					KeyValue::new("destination", msg.dest.kind()),
					KeyValue::new("kind", kind),
				]);
		}

		debug_assert!(!sender.is_full(), "channel full");
		debug_assert!(!sender.is_closed(), "channel closed");
		sender.send(msg).map_err(|e| err!("{e}"))
	}

	/// Hands the oldest unsent request of each destination to its worker, so
	/// a backlog with nothing in flight is sent without waiting for a new
	/// event. Bypasses `dispatch` as these requests were already counted.
	async fn dispatch_queued(&self, shard: Option<usize>) {
		for (dest, (queue_id, event)) in self.db.queued_heads().await {
			let id = self.shard_id(&dest);
			if shard.is_none_or(|shard| shard == id) {
				self.channels[id].0.send(Msg { dest, event, queue_id }).ok();
			}
		}
	}

	pub fn queue_depth(&self) -> usize {
		self.channels.iter().map(|(sender, _)| sender.len()).sum()
	}

	pub fn workers_alive(&self) -> usize { self.workers_alive.load(Ordering::Relaxed) }

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

/// Delay before restarting a worker after its `failures`-th consecutive
/// failure, doubling each time.
fn restart_delay(failures: u32) -> Duration {
	WORKER_RESTART_DELAY
		.saturating_mul(2_u32.saturating_pow(failures.saturating_sub(1)))
		.min(WORKER_RESTART_DELAY_MAX)
}
