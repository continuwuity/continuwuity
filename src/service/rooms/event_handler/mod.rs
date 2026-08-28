mod acl_check;
mod fetch_and_handle_outliers;
mod fetch_auth;
mod fetch_prev;
mod fetch_state;
mod handle_incoming_pdu;
mod handle_outlier_pdu;
mod parse_incoming_pdu;
pub mod pdu_checks;
mod pending_join_pdus;
mod policy_server;
mod resolve_state;
mod state_at_incoming;
mod upgrade_outlier_pdu;

use std::{collections::HashMap, fmt::Write, sync::Arc, time::Instant};
use std::time::Duration;
use assign::assign;
use async_trait::async_trait;
use http::StatusCode;
use conduwuit::{utils::MutexMap, Err, Error, Event, PduEvent, Result, Server, SyncRwLock};
pub use fetch_and_handle_outliers::{
	DagBuilderTree, GET_MISSING_EVENTS_MAX_BATCH_SIZE, build_local_dag,
};
use ruma::{
	EventId, OwnedEventId, OwnedRoomId, OwnedServerName,
	api::error::{ErrorKind, LimitExceededErrorData, RetryAfter},
	events::room::create::RoomCreateEventContent,
	room_version_rules::RoomVersionRules,
};
use serde_json::value::RawValue as RawJsonValue;
use tokio::sync::{Notify, mpsc};

use crate::{Dep, globals, rooms, sending, server_keys};

pub type FailedPDUPull = (u32, Instant);  // (pull count, last retry)

pub struct Service {
	pub mutex_federation: RoomMutexMap,
	pub federation_handletime: SyncRwLock<HandleTimeMap>,
	pub extremity_squashers: SyncRwLock<HashMap<OwnedRoomId, mpsc::Sender<(usize, bool)>>>,
	pub failed_pdu_pulls: SyncRwLock<HashMap<OwnedEventId, FailedPDUPull>>,
	joining_rooms: SyncRwLock<HashMap<OwnedRoomId, mpsc::Sender<PendingJoinPdu>>>,
	services: Services,
	server_shutdown: Notify,
	me: std::sync::Weak<Self>,
}

struct Services {
	globals: Dep<globals::Service>,
	sending: Dep<sending::Service>,
	auth_chain: Dep<rooms::auth_chain::Service>,
	metadata: Dep<rooms::metadata::Service>,
	outlier: Dep<rooms::outlier::Service>,
	pdu_metadata: Dep<rooms::pdu_metadata::Service>,
	server_keys: Dep<server_keys::Service>,
	short: Dep<rooms::short::Service>,
	state: Dep<rooms::state::Service>,
	state_cache: Dep<rooms::state_cache::Service>,
	state_accessor: Dep<rooms::state_accessor::Service>,
	state_compressor: Dep<rooms::state_compressor::Service>,
	timeline: Dep<rooms::timeline::Service>,
	server: Arc<Server>,
}

type RoomMutexMap = MutexMap<OwnedRoomId, ()>;
type HandleTimeMap = HashMap<OwnedRoomId, (OwnedEventId, Instant)>;

enum PendingJoinPdu {
	Pdu(OwnedServerName, Box<RawJsonValue>),
	Complete,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new_cyclic(|s| Self {
			me: s.clone(),
			mutex_federation: RoomMutexMap::new(),
			federation_handletime: HandleTimeMap::new().into(),
			extremity_squashers: SyncRwLock::new(HashMap::new()),
			failed_pdu_pulls: SyncRwLock::new(HashMap::new()),
			joining_rooms: HashMap::new().into(),
			services: Services {
				globals: args.depend::<globals::Service>("globals"),
				sending: args.depend::<sending::Service>("sending"),
				auth_chain: args.depend::<rooms::auth_chain::Service>("rooms::auth_chain"),
				metadata: args.depend::<rooms::metadata::Service>("rooms::metadata"),
				outlier: args.depend::<rooms::outlier::Service>("rooms::outlier"),
				server_keys: args.depend::<server_keys::Service>("server_keys"),
				pdu_metadata: args.depend::<rooms::pdu_metadata::Service>("rooms::pdu_metadata"),
				short: args.depend::<rooms::short::Service>("rooms::short"),
				state: args.depend::<rooms::state::Service>("rooms::state"),
				state_cache: args.depend::<rooms::state_cache::Service>("rooms::state_cache"),
				state_accessor: args
					.depend::<rooms::state_accessor::Service>("rooms::state_accessor"),
				state_compressor: args
					.depend::<rooms::state_compressor::Service>("rooms::state_compressor"),
				timeline: args.depend::<rooms::timeline::Service>("rooms::timeline"),
				server: args.server.clone(),
			},
			server_shutdown: Notify::new(),
		}))
	}

	async fn memory_usage(&self, out: &mut (dyn Write + Send)) -> Result {
		let mutex_federation = self.mutex_federation.len();
		writeln!(out, "federation_mutex: {mutex_federation}")?;

		let federation_handletime = self.federation_handletime.read().len();
		writeln!(out, "federation_handletime: {federation_handletime}")?;

		Ok(())
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }

	fn interrupt(&self) { self.server_shutdown.notify_waiters(); }

	async fn clear_cache(&self) {
		let mut failed_pdu_pulls = self.failed_pdu_pulls.write();
		failed_pdu_pulls.clear();
	}
}

impl Service {
	/// Checks if a single event exists. Alias for
	/// `self.services.timeline.pdu_exists`.
	async fn event_exists(&self, event_id: OwnedEventId) -> bool {
		self.services.timeline.pdu_exists(&event_id).await
	}

	/// Fetches a single PDU, returning None if there is an error.
	async fn event_fetch(&self, event_id: OwnedEventId) -> Option<PduEvent> {
		self.services.timeline.get_pdu(&event_id).await.ok()
	}

	/// Returns a rate-limit error if the requested event had a recent failed pull attempt.
	///
	/// If the event is not being backed off from, `Ok(())` is returned.
	pub(super) fn ensure_can_pull_event(&self, event_id: &EventId) -> Result<()> {
		let map = self.failed_pdu_pulls.read();
		let Some((retry_count, last_retry)) = map.get(event_id) else {
			return Ok(());
		};
		let min = Duration::from_secs(self.services.server.config.sender_retry_backoff_base);
		let max = Duration::from_secs(self.services.server.config.sender_retry_backoff_limit);
		let next_retry = min.saturating_mul(*retry_count).min(max);
		if last_retry.elapsed() >= next_retry {
			Ok(())
		} else {
			Err(Error::Request(
				ErrorKind::LimitExceeded(assign!(LimitExceededErrorData::new(), {
					retry_after: Some(RetryAfter::Delay(next_retry)),
				})),
				format!("Event {event_id} failed to be fetched recently, backing off").into(),
				StatusCode::TOO_MANY_REQUESTS,
			))
		}
	}

	/// Marks a PDU as having a failed pull attempt, preventing it from being immediately re-fetched without a short cooldown.
	pub(super) fn hit_failed_pdu_pull(&self, event_id: OwnedEventId) {
		let now = Instant::now();
		let mut map = self.failed_pdu_pulls.write();
		map.entry(event_id).and_modify(|(retries, last_retry)| {
			if *last_retry > now {
				return;
			}
			*retries = retries.saturating_add(1);
			*last_retry = Instant::now();
		}).or_insert_with(|| (1, Instant::now()));
	}

	/// Removes a PDU from the failed pulls map, allowing it to be re-fetched in future if needed.
	pub(super) fn clear_failed_pdu(&self, event_id: &EventId) {
		// NOTE: this is a bit pointless since we're unlikely to try pulling an event again if we fetch it successfully, but it doesn't hurt to have to prevent the map growing indefinitely.
		let mut map = self.failed_pdu_pulls.write();
		map.remove(event_id);
	}
}

fn get_room_version_rules<Pdu: Event>(create_event: &Pdu) -> Result<RoomVersionRules> {
	let content: RoomCreateEventContent = create_event.get_content()?;
	let Some(room_version_rules) = content.room_version.rules() else {
		return Err!(Request(UnsupportedRoomVersion("Room version has no defined rules")));
	};

	Ok(room_version_rules)
}
