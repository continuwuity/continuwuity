use std::sync::Arc;

use conduwuit_core::utils::stream::TryIgnore;
use conduwuit_database::{Database, Deserialized, Json, Map};
use futures::StreamExt;
use tower_sessions::{
	ExpiredDeletion, SessionStore,
	cookie::time::OffsetDateTime,
	session::{Id, Record},
	session_store::Error,
};

#[derive(Debug, Clone)]
pub(crate) struct RocksDbSessionStore {
	websessionid_session: Arc<Map>,
}

impl RocksDbSessionStore {
	pub(crate) fn new(db: &Database) -> Self {
		Self {
			websessionid_session: db["websessionid_session"].clone(),
		}
	}
}

#[async_trait::async_trait]
impl SessionStore for RocksDbSessionStore {
	async fn save(&self, session: &Record) -> Result<(), Error> {
		self.websessionid_session
			.raw_put(session.id.0.to_be_bytes(), Json(session));

		Ok(())
	}

	async fn load(&self, session_id: &Id) -> Result<Option<Record>, Error> {
		let Some(session) = self
			.websessionid_session
			.get(&session_id.0.to_be_bytes())
			.await
			.deserialized()
			.ok()
		else {
			return Ok(None);
		};

		Ok(Some(session))
	}

	async fn delete(&self, session_id: &Id) -> Result<(), Error> {
		self.websessionid_session
			.remove(&session_id.0.to_be_bytes());

		Ok(())
	}
}

#[async_trait::async_trait]
impl ExpiredDeletion for RocksDbSessionStore {
	async fn delete_expired(&self) -> Result<(), Error> {
		let now = OffsetDateTime::now_utc();

		self.websessionid_session
			.stream()
			.ignore_err()
			.for_each(async |(id, session): (&[u8], Record)| {
				if session.expiry_date < now {
					self.websessionid_session.remove(id);
				}
			})
			.await;

		Ok(())
	}
}
