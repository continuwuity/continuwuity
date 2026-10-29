use std::{
	sync::Arc,
	time::{Duration, SystemTime},
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
	created_at: SystemTime,
	last_sequence_token: String,
	transaction_sequences: LruCache<OwnedTransactionId, String>,
}

impl Session {
	const SAVED_TRANSACTION_SEQUENCES: usize = 256;

	fn new(data: String, sequence_token: String) -> Self {
		Self {
			data,
			created_at: SystemTime::now(),
			last_sequence_token: sequence_token,
			transaction_sequences: LruCache::new(Self::SAVED_TRANSACTION_SEQUENCES),
		}
	}
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

	fn lock_session(&self, id: &str) -> Result<SyncMappedMutexGuard<'_, Session>> {
		SyncMutexGuard::try_map_or_err(self.sessions.lock(), |sessions| {
			sessions
				.get_mut(id)
				.filter(|session| {
					SystemTime::now()
						.duration_since(session.created_at)
						.is_ok_and(|duration| duration < Self::SESSION_MAX_AGE)
				})
				.ok_or_else(|| err!(Request(NotFound("Unknown session"))))
		})
		.map_err(at!(1))
	}

	pub fn create_session(
		&self,
		data: String,
	) -> Result<create_rendezvous_session::unstable_msc4388::Response> {
		if data.len() > Self::MAX_DATA_SIZE_BYTES {
			return Err!(Request(TooLarge("Data too large")));
		}

		let id = Self::create_token();
		let sequence_token = Self::create_token();

		self.sessions
			.lock()
			.insert(id.clone(), Session::new(data, sequence_token.clone()));

		Ok(create_rendezvous_session::unstable_msc4388::Response::new(
			id,
			sequence_token,
			Self::SESSION_MAX_AGE,
		))
	}

	pub fn update_session(
		&self,
		id: &str,
		transaction: OwnedTransactionId,
		sequence_token: String,
		data: String,
	) -> Result<update_rendezvous_session::unstable::Response> {
		if data.len() > Self::MAX_DATA_SIZE_BYTES {
			return Err!(Request(TooLarge("Data too large")));
		}

		let mut session = self.lock_session(id)?;

		if let Some(sequence_token) = session.transaction_sequences.get_mut(&transaction) {
			return Ok(update_rendezvous_session::unstable::Response::new(
				sequence_token.to_owned(),
			));
		}

		if session.last_sequence_token != sequence_token {
			return Err!(Request(ConcurrentWrite("Sequence token mismatch")));
		}

		let new_sequence_token = Self::create_token();

		session.data = data;
		session.last_sequence_token.clone_from(&new_sequence_token);
		session
			.transaction_sequences
			.insert(transaction, sequence_token);

		Ok(update_rendezvous_session::unstable::Response::new(new_sequence_token))
	}

	pub fn get_session(&self, id: &str) -> Result<get_rendezvous_session::unstable::Response> {
		let session = self.lock_session(id)?;

		let session_age = SystemTime::now()
			.duration_since(session.created_at)
			.unwrap_or(Duration::MAX);

		let session_time_remaining = Self::SESSION_MAX_AGE.saturating_sub(session_age);

		Ok(get_rendezvous_session::unstable::Response::new(
			session.last_sequence_token.clone(),
			session.data.clone(),
			session_time_remaining,
		))
	}

	pub fn delete_session(
		&self,
		id: &str,
	) -> Result<delete_rendezvous_session::unstable::Response> {
		let mut sessions = self.sessions.lock();

		if sessions.remove(id).is_some() {
			Ok(delete_rendezvous_session::unstable::Response::new())
		} else {
			Err!(Request(NotFound("Unknown session")))
		}
	}
}
