use std::{
	sync::Arc,
	time::{Duration, Instant},
};

use conduwuit::{Err, Result, SyncMappedMutexGuard, SyncMutex, SyncMutexGuard, at, err, utils};
use lru_cache::LruCache;
use ruma::{
	OwnedTransactionId,
	api::client::rendezvous::{
		create_rendezvous_session, delete_rendezvous_session, get_rendezvous_session,
		update_rendezvous_session,
	},
};

pub struct Service {
	sessions: SyncMutex<LruCache<String, Session>>,
}

struct Session {
	data: String,
	created_at: Instant,
	last_sequence_token: String,
	transaction_sequences: LruCache<OwnedTransactionId, String>,
}

impl Session {
	const SAVED_TRANSACTION_SEQUENCES: usize = 256;

	fn new(data: String, sequence_token: String, now: Instant) -> Self {
		Self {
			data,
			created_at: now,
			last_sequence_token: sequence_token,
			transaction_sequences: LruCache::new(Self::SAVED_TRANSACTION_SEQUENCES),
		}
	}

	fn time_remaining(&self, now: Instant) -> Option<Duration> {
		let age = now.saturating_duration_since(self.created_at);
		(age < Service::SESSION_MAX_AGE)
			.then(|| Service::SESSION_MAX_AGE.saturating_sub(age))
			.filter(|remaining| !remaining.is_zero())
	}

	fn is_expired(&self, now: Instant) -> bool { self.time_remaining(now).is_none() }
}

impl crate::Service for Service {
	fn build(_: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			sessions: SyncMutex::new(LruCache::new(Self::MAX_SESSIONS)),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	const MAX_DATA_SIZE_BYTES: usize = 4096;
	const MAX_SESSIONS: usize = 256;
	const SESSION_MAX_AGE: Duration = Duration::from_secs(300);
	const TOKEN_LENGTH: usize = 32;

	fn create_token() -> String { utils::random_string(Self::TOKEN_LENGTH) }

	fn is_valid_opaque_id(id: &str) -> bool {
		!id.is_empty()
			&& id.len() <= 255
			&& id
				.bytes()
				.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~'))
	}

	/// Lock and fetch an unexpired rendezvous session.
	fn lock_session(&self, id: &str) -> Result<SyncMappedMutexGuard<'_, Session>> {
		self.lock_session_at(id, Instant::now())
	}

	fn lock_session_at(
		&self,
		id: &str,
		now: Instant,
	) -> Result<SyncMappedMutexGuard<'_, Session>> {
		// We don't clear out expired sessions manually, the LRU cache will
		// evict them over time
		SyncMutexGuard::try_map_or_err(self.sessions.lock(), |sessions| {
			sessions
				.get_mut(id)
				.filter(|session| !session.is_expired(now))
				.ok_or_else(|| err!(Request(NotFound("Unknown session"))))
		})
		.map_err(at!(1))
	}

	/// Create a fresh rendezvous session.
	pub fn create_session(
		&self,
		data: String,
	) -> Result<create_rendezvous_session::msc4388::Response> {
		if data.len() > Self::MAX_DATA_SIZE_BYTES {
			return Err!(Request(TooLarge("Data too large")));
		}

		let id = Self::create_token();
		let sequence_token = Self::create_token();

		self.sessions
			.lock()
			.insert(id.clone(), Session::new(data, sequence_token.clone(), Instant::now()));

		Ok(create_rendezvous_session::msc4388::Response::new(
			id,
			sequence_token,
			Self::SESSION_MAX_AGE,
		))
	}

	/// Update a rendezvous session with new data.
	pub fn update_session(
		&self,
		id: &str,
		transaction: OwnedTransactionId,
		sequence_token: String,
		data: String,
	) -> Result<update_rendezvous_session::msc4388::Response> {
		if !Self::is_valid_opaque_id(transaction.as_str()) {
			return Err!(Request(InvalidParam(
				"Transaction ID does not comply with the opaque identifier grammar"
			)));
		}

		let mut session = self.lock_session(id)?;

		// If the provided transaction ID was used already, we have to return
		// the sequence token that we returned last time it was used.
		// This makes session updates with the same transaction ID idempotent.
		if let Some(sequence_token) = session.transaction_sequences.get_mut(&transaction) {
			return Ok(update_rendezvous_session::msc4388::Response::new(
				sequence_token.to_owned(),
			));
		}

		if session.last_sequence_token != sequence_token {
			return Err!(Request(ConcurrentWrite("Sequence token mismatch")));
		}

		drop(sequence_token);

		let new_sequence_token = Self::create_token();

		if data.len() > Self::MAX_DATA_SIZE_BYTES {
			return Err!(Request(TooLarge("Data too large")));
		}

		session.data = data;
		session.last_sequence_token.clone_from(&new_sequence_token);
		session
			.transaction_sequences
			.insert(transaction, new_sequence_token.clone());

		Ok(update_rendezvous_session::msc4388::Response::new(new_sequence_token))
	}

	/// Look up a rendezvous session by ID.
	pub fn get_session(&self, id: &str) -> Result<get_rendezvous_session::msc4388::Response> {
		let now = Instant::now();
		let session = self.lock_session_at(id, now)?;

		let session_time_remaining = session
			.time_remaining(now)
			.unwrap_or_else(|| Duration::from_millis(1));

		Ok(get_rendezvous_session::msc4388::Response::new(
			session.last_sequence_token.clone(),
			session.data.clone(),
			session_time_remaining,
		))
	}

	/// Delete a rendezvous session.
	pub fn delete_session(
		&self,
		id: &str,
	) -> Result<delete_rendezvous_session::msc4388::Response> {
		let mut sessions = self.sessions.lock();
		let now = Instant::now();

		if sessions
			.remove(id)
			.is_some_and(|session| !session.is_expired(now))
		{
			Ok(delete_rendezvous_session::msc4388::Response::new())
		} else {
			Err!(Request(NotFound("Unknown session")))
		}
	}
}
