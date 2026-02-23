use std::{
	collections::HashMap,
	fmt,
	sync::{
		Arc,
		atomic::{AtomicU64, Ordering},
	},
	time::{Duration, SystemTime},
};

use async_trait::async_trait;
use conduwuit::{Error, Result, SyncRwLock, debug_warn, warn};
use database::{Handle, Map};
use futures::SinkExt;
use ruma::{
	DeviceId, OwnedServerName, OwnedTransactionId, TransactionId, UserId,
	api::{
		client::error::ErrorKind::LimitExceeded,
		federation::transactions::send_transaction_message,
	},
};
use tokio::sync::watch::{Receiver, Sender};

use crate::{Dep, config};

pub type TxnKey = (OwnedServerName, OwnedTransactionId);
pub type WrappedTransactionResponse =
	Option<Result<send_transaction_message::v1::Response, TransactionError>>;

/// Errors that can occur during federation transaction processing.
#[derive(Debug, Clone)]
pub enum TransactionError {
	/// Server is shutting down - the sender should retry the entire
	/// transaction.
	ShuttingDown,
}

impl fmt::Display for TransactionError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			| Self::ShuttingDown => write!(f, "Server is shutting down"),
		}
	}
}

impl std::error::Error for TransactionError {}

/// Minimum interval between cache cleanup runs.
/// Exists to prevent thrashing when the cache is full of things that can't be
/// cleared
const CLEANUP_INTERVAL_SECS: u64 = 30;

#[derive(Clone, Debug)]
pub struct CachedTxnResponse {
	pub response: send_transaction_message::v1::Response,
	pub created: SystemTime,
}

/// Internal state for a federation transaction.
/// Either actively being processed or completed and cached.
#[derive(Clone)]
enum TxnState {
	/// Transaction is currently being processed.
	Active(Receiver<WrappedTransactionResponse>),

	/// Transaction completed and response is cached.
	Cached(CachedTxnResponse),
}

/// Result of atomically checking or starting a federation transaction.
pub enum FederationTxnState {
	/// Transaction already completed and cached
	Cached(send_transaction_message::v1::Response),

	/// Transaction is currently being processed by another request.
	/// Wait on this receiver for the result.
	Active(Receiver<WrappedTransactionResponse>),

	/// This caller should process the transaction (first to request it).
	Started {
		receiver: Receiver<WrappedTransactionResponse>,
		sender: Sender<WrappedTransactionResponse>,
	},
}

pub struct Service {
	services: Services,
	db: Data,
	federation_txn_state: Arc<SyncRwLock<HashMap<TxnKey, TxnState>>>,
	last_cleanup: AtomicU64,
}

struct Services {
	config: Dep<config::Service>,
}

struct Data {
	userdevicetxnid_response: Arc<Map>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				config: args.depend::<config::Service>("config"),
			},
			db: Data {
				userdevicetxnid_response: args.db["userdevicetxnid_response"].clone(),
			},
			federation_txn_state: Arc::new(SyncRwLock::new(HashMap::new())),
			last_cleanup: AtomicU64::new(0),
		}))
	}

	async fn clear_cache(&self) {
		let mut state = self.federation_txn_state.write();
		// Only clear cached entries, preserve active transactions
		state.retain(|_, v| matches!(v, TxnState::Active(_)));
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Returns the count of currently active (in-progress) transactions.
	#[must_use]
	pub fn txn_active_handle_count(&self) -> usize {
		let state = self.federation_txn_state.read();
		state
			.values()
			.filter(|v| matches!(v, TxnState::Active(_)))
			.count()
	}

	pub fn add_client_txnid(
		&self,
		user_id: &UserId,
		device_id: Option<&DeviceId>,
		txn_id: &TransactionId,
		data: &[u8],
	) {
		let mut key = user_id.as_bytes().to_vec();
		key.push(0xFF);
		key.extend_from_slice(device_id.map(DeviceId::as_bytes).unwrap_or_default());
		key.push(0xFF);
		key.extend_from_slice(txn_id.as_bytes());

		self.db.userdevicetxnid_response.insert(&key, data);
	}

	pub async fn get_client_txn(
		&self,
		user_id: &UserId,
		device_id: Option<&DeviceId>,
		txn_id: &TransactionId,
	) -> Result<Handle<'_>> {
		let key = (user_id, device_id, txn_id);
		self.db.userdevicetxnid_response.qry(&key).await
	}

	/// Atomically gets a cached response, joins an active transaction, or
	/// starts a new one.
	pub fn get_or_start_federation_txn(&self, key: TxnKey) -> Result<FederationTxnState> {
		// Only one upgradable lock can be held at a time, and there aren't any
		// read-only locks, so no point being upgradable
		let mut state = self.federation_txn_state.write();

		// Check existing state for this key
		if let Some(txn_state) = state.get(&key) {
			return Ok(match txn_state {
				| TxnState::Cached(cached) => FederationTxnState::Cached(cached.response.clone()),
				| TxnState::Active(receiver) => FederationTxnState::Active(receiver.clone()),
			});
		}

		// Check if another transaction from this origin is already running
		let has_active_from_origin = state
			.iter()
			.any(|(k, v)| k.0 == key.0 && matches!(v, TxnState::Active(_)));

		if has_active_from_origin {
			debug_warn!(
				origin = ?key.0,
				"Got concurrent transaction request from an origin with an active transaction"
			);
			return Err(Error::BadRequest(
				LimitExceeded { retry_after: None },
				"Still processing another transaction from this origin",
			));
		}

		let max_active_txns = self.services.config.max_concurrent_inbound_transactions;

		// Check if we're at capacity
		if state.len() >= max_active_txns
			&& let active_count = state
				.values()
				.filter(|v| matches!(v, TxnState::Active(_)))
				.count() && active_count >= max_active_txns
		{
			warn!(
				active = active_count,
				max = max_active_txns,
				"Server is overloaded, dropping incoming transaction"
			);
			return Err(Error::BadRequest(
				LimitExceeded { retry_after: None },
				"Server is overloaded, try again later",
			));
		}

		// Start new transaction
		let (sender, receiver) = tokio::sync::watch::channel(None);
		state.insert(key, TxnState::Active(receiver.clone()));

		Ok(FederationTxnState::Started { receiver, sender })
	}

	/// Finishes a transaction by transitioning it from active to cached state.
	/// Additionally may trigger cleanup of old entries.
	pub fn finish_federation_txn(
		&self,
		key: TxnKey,
		sender: Sender<WrappedTransactionResponse>,
		response: send_transaction_message::v1::Response,
	) {
		// Check if cleanup might be needed before acquiring the lock
		let should_try_cleanup = self.should_try_cleanup();

		let mut state = self.federation_txn_state.write();

		// Explicitly set cached first so there is no gap where receivers get a closed
		// channel
		state.insert(
			key,
			TxnState::Cached(CachedTxnResponse {
				response: response.clone(),
				created: SystemTime::now(),
			}),
		);

		if let Err(e) = sender.send(Some(Ok(response))) {
			debug_warn!("Failed to send transaction response to waiting receivers: {e}");
		}

		// Explicitly close
		drop(sender);

		// This task is dangling, we can try clean caches now
		if should_try_cleanup {
			self.cleanup_entries_locked(&mut state);
		}
	}

	pub fn remove_federation_txn(&self, key: &TxnKey) {
		let mut state = self.federation_txn_state.write();
		state.remove(key);
	}

	/// Checks if enough time has passed since the last cleanup to consider
	/// running another. Updates the last cleanup time if returning true.
	fn should_try_cleanup(&self) -> bool {
		let now = SystemTime::now()
			.duration_since(SystemTime::UNIX_EPOCH)
			.expect("SystemTime before UNIX_EPOCH")
			.as_secs();
		let last = self.last_cleanup.load(Ordering::Relaxed);

		if now.saturating_sub(last) >= CLEANUP_INTERVAL_SECS {
			// CAS: only update if no one else has updated it since we read
			self.last_cleanup
				.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
				.is_ok()
		} else {
			false
		}
	}

	/// Cleans up cached entries based on age and count limits.
	///
	/// First removes all cached entries older than the configured max age.
	/// Then, if the cache still exceeds the max entry count, removes the oldest
	/// cached entries until the count is within limits.
	///
	/// Must be called with write lock held on the state map.
	fn cleanup_entries_locked(&self, state: &mut HashMap<TxnKey, TxnState>) {
		let max_age_secs = self.services.config.transaction_id_cache_max_age_secs;
		let max_entries = self.services.config.transaction_id_cache_max_entries;

		// First pass: remove all cached entries older than max age
		let cutoff = SystemTime::now()
			.checked_sub(Duration::from_secs(max_age_secs))
			.unwrap_or(SystemTime::UNIX_EPOCH);

		state.retain(|_, v| match v {
			| TxnState::Active(_) => true, // Never remove active transactions
			| TxnState::Cached(cached) => cached.created > cutoff,
		});

		// Count cached entries
		let cached_count = state
			.values()
			.filter(|v| matches!(v, TxnState::Cached(_)))
			.count();

		// Second pass: if still over max entries, remove oldest cached entries
		if cached_count > max_entries {
			let excess = cached_count.saturating_sub(max_entries);

			// Collect cached entries sorted by age (oldest first)
			let mut cached_entries: Vec<_> = state
				.iter()
				.filter_map(|(k, v)| match v {
					| TxnState::Cached(cached) => Some((k.clone(), cached.created)),
					| TxnState::Active(_) => None,
				})
				.collect();
			cached_entries.sort_by(|a, b| a.1.cmp(&b.1));

			// Remove the oldest cached entries to get under the limit
			for (key, _) in cached_entries.into_iter().take(excess) {
				state.remove(&key);
			}
		}
	}
}
