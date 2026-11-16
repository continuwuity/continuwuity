use std::{
	collections::{HashMap, hash_map::Entry},
	sync::Arc,
	time::{Duration, Instant},
};

use async_trait::async_trait;
use conduwuit::{Error, Server, debug, err, error, utils::millis_since_unix_epoch};
use opentelemetry::KeyValue;
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, OwnedEventId, OwnedRoomId, OwnedServerName,
	api::error::ErrorKind, events::TimelineEventType,
};
use tokio::{
	sync::{
		Notify, mpsc,
		mpsc::{Sender, UnboundedReceiver, UnboundedSender},
	},
	task::JoinSet,
};

use crate::{
	rooms,
	service::{Args, Dep},
};

pub struct Services {
	event_handler: Dep<rooms::event_handler::Service>,
	server: Arc<Server>,
}

pub struct Service {
	queues: conduwuit::SyncMutex<HashMap<OwnedRoomId, UnboundedSender<QueueJob>>>,
	send: Sender<QueueJob>,
	recv: tokio::sync::Mutex<mpsc::Receiver<QueueJob>>,
	shutdown: Notify,
	services: Services,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: Args<'_>) -> conduwuit::Result<Arc<Self>>
	where
		Self: Sized,
	{
		let (send, recv) = mpsc::channel(1);
		Ok(Arc::new(Self {
			queues: conduwuit::SyncMutex::new(HashMap::new()),
			shutdown: Notify::new(),
			send,
			recv: tokio::sync::Mutex::new(recv),
			services: Services {
				event_handler: args.depend("rooms::event_handler"),
				server: args.server.clone(),
			},
		}))
	}

	async fn worker(self: Arc<Self>) -> conduwuit::Result<()> {
		let mut recv = self.recv.lock().await;
		let mut handles = JoinSet::new();
		let (yielder, mut yielding) = mpsc::channel(5);

		loop {
			let job = tokio::select! {
				() = self.shutdown.notified() => {
					yielding.close();
					break;
				},
				room_id = yielding.recv() => {
					if let Some(room_id) = room_id {
						self.queues.lock().remove(&room_id);
					}
					continue;
				}
				job = recv.recv() => job
			};
			let Some(job) = job else {
				conduwuit::debug!("No job (channel closed)");
				break;
			};

			let mut queues = self.queues.lock();
			match queues.entry(job.room_id.clone()) {
				| Entry::Occupied(entry) => {
					entry.get().send(job).unwrap();
					// TODO: handle error
					// It might be possible for a self-reaping worker to close
					// at the same time we try to send to it, which will cause
					// this send to return a "channel closed" error.
				},
				| Entry::Vacant(entry) => {
					let room_id = entry.key().to_owned();
					let (sender, receiver) = mpsc::unbounded_channel();
					sender.send(job).unwrap(); // TODO: handle error

					handles.spawn_on(
						Self::room_worker(self.clone(), room_id, receiver, yielder.clone()),
						self.services.server.runtime(),
					);
					entry.insert(sender);
				},
			}
		}

		// If the loop broke, make sure that the shutdown signal was sent, so
		// that any room workers are guaranteed to shut down as soon as
		// possible.
		self.interrupt();

		while let Some(ret) = handles.join_next_with_id().await {
			match ret {
				| Ok((id, ())) => {
					debug!(?id, "room queue worker finished");
				},
				| Err(error) => {
					error!(id = ?error.id(), ?error, "room queue worker failed");
				},
			}
		}

		Ok(())
	}

	fn interrupt(&self) { self.shutdown.notify_waiters(); }

	fn name(&self) -> &'static str { module_path!() }
}

impl Service {
	pub async fn new_job(&self, job: QueueJob) -> conduwuit::Result<()> {
		self.send
			.send(job)
			.await
			.map_err(|e| err!("failed to send PDU to worker: {e}"))
	}

	#[tracing::instrument(skip(self,channel,yielder), fields(%room_id))]
	async fn room_worker(
		self: Arc<Self>,
		room_id: OwnedRoomId,
		mut channel: UnboundedReceiver<QueueJob>,
		yielder: Sender<OwnedRoomId>,
	) {
		loop {
			let job = tokio::select! {
				() = self.shutdown.notified() => {
					channel.close();
					return;
				},
				() = tokio::time::sleep(Duration::from_mins(5)) => {
					yielder.send(room_id).await.ok();
					return;
				},
				job = channel.recv() => job
			};
			let Some(job) = job else {
				conduwuit::debug!("No job (channel closed)");
				break;
			};
			conduwuit::debug!(event_id=%job.event_id, origin=%job.origin, "Handling incoming job");

			let event_type = KeyValue::new(
				"type",
				pdu_type_label(job.object.get("type").and_then(CanonicalJsonValue::as_str)),
			);
			let origin_server_ts = job
				.object
				.get("origin_server_ts")
				.and_then(CanonicalJsonValue::as_integer)
				.and_then(|ts| u64::try_from(i64::from(ts)).ok());

			let start = Instant::now();

			let result = self
				.services
				.event_handler
				.handle_incoming_pdu(&job.origin, &job.room_id, &job.event_id, job.object, false)
				.await;

			let outcome = KeyValue::new("outcome", match &result {
				| Ok(_) => "accepted",
				| Err(Error::BadRequest(ErrorKind::Forbidden, ..)) => "forbidden",
				| Err(Error::BadRequest(ErrorKind::InvalidParam | ErrorKind::BadJson, ..)) =>
					"invalid",
				| Err(_) => "error",
			});
			let instruments = &self.services.server.metrics.instruments;
			instruments
				.federation_incoming_pdu_duration
				.record(start.elapsed().as_secs_f64(), std::slice::from_ref(&outcome));
			if let (Ok(_), Some(ts)) = (&result, origin_server_ts) {
				let lag = millis_since_unix_epoch().saturating_sub(ts);
				instruments
					.federation_incoming_pdu_lag
					.record(Duration::from_millis(lag).as_secs_f64(), &[]);
			}
			instruments
				.federation_incoming_pdus
				.add(1, &[event_type, outcome]);
		}
	}
}

pub struct QueueJob {
	pub origin: OwnedServerName,
	pub event_id: OwnedEventId,
	pub room_id: OwnedRoomId,
	pub object: CanonicalJsonObject,
}

/// Known event types, or `other`, so remote servers cannot create new
/// attribute values.
fn pdu_type_label(event_type: Option<&str>) -> String {
	match event_type.map(TimelineEventType::from) {
		| Some(TimelineEventType::_Custom(_)) | None => "other".to_owned(),
		| Some(event_type) => event_type.to_string(),
	}
}
