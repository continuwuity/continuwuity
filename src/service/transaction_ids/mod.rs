use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use conduwuit::{Error, Result, SyncRwLock};
use database::{Handle, Map};
use ruma::{
	DeviceId, OwnedServerName, OwnedTransactionId, TransactionId, UserId,
	api::{
		client::error::ErrorKind::LimitExceeded,
		federation::transactions::send_transaction_message,
	},
};
use tokio::sync::watch::{Receiver, Sender};

pub type TxnKey = (OwnedServerName, OwnedTransactionId);
pub type WrappedTransactionResponse = Option<send_transaction_message::v1::Response>;
pub type ActiveTransactionsMap = HashMap<TxnKey, Receiver<WrappedTransactionResponse>>;

pub struct Service {
	db: Data,
	servername_txnid_response_cache:
		Arc<SyncRwLock<HashMap<TxnKey, send_transaction_message::v1::Response>>>,
	servername_txnid_active: Arc<SyncRwLock<ActiveTransactionsMap>>,
	max_active_txns: usize,
}

struct Data {
	userdevicetxnid_response: Arc<Map>,
}

#[async_trait]
impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				userdevicetxnid_response: args.db["userdevicetxnid_response"].clone(),
			},
			servername_txnid_response_cache: Arc::new(SyncRwLock::new(HashMap::new())),
			servername_txnid_active: Arc::new(SyncRwLock::new(HashMap::new())),
			max_active_txns: args.depend::<crate::config::Service>("config").max_concurrent_inbound_transactions
		}))
	}

	async fn clear_cache(&self) {
		let mut state = self.servername_txnid_response_cache.write();
		state.clear();
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
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

	/// Fetches a receiver channel for the given transaction, if any exists.
	/// If the given txn is not active, None is returned.
	#[must_use]
	pub fn get_active_federation_txn(
		&self,
		key: &TxnKey,
	) -> Option<Receiver<WrappedTransactionResponse>> {
		let state = self.servername_txnid_active.read();
		state.get(key).cloned()
	}

	/// Starts a new inbound transaction handler, returning the appropriate
	/// sender to broadcast the response via.
	///
	/// If the given key is already active, a rate-limited response is returned.
	pub fn start_federation_txn(
		&self,
		key: TxnKey,
	) -> Result<Sender<WrappedTransactionResponse>> {
		let mut state = self.servername_txnid_active.write();
		if state.get(&key).is_some() {
			Err(Error::BadRequest(
				LimitExceeded { retry_after: None },
				"Transaction is already being handled",
			))
		} else if state.keys().any(|k| k.0 == key.0) {
			Err(Error::BadRequest(
				LimitExceeded { retry_after: None },
				"Still processing another transaction from this origin",
			))
		} else if state.len() >= self.max_active_txns {
			Err(Error::BadRequest(
				LimitExceeded { retry_after: None },
				"Server is overloaded, try again later",
			))
		} else {
			let (tx, rx) = tokio::sync::watch::channel(None);
			state.insert(key, rx);
			Ok(tx)
		}
	}

	/// Finishes a transaction, removing it from the active txns registry.
	pub fn finish_federation_txn(&self, key: &TxnKey) {
		let mut state = self.servername_txnid_active.write();
		state.remove(key);
	}

	/// Gets a cached transaction response, if the given key has a value.
	#[must_use]
	pub fn get_cached_txn(&self, key: &TxnKey) -> Option<send_transaction_message::v1::Response> {
		let state = self.servername_txnid_response_cache.read();
		state.get(key).cloned()
	}

	/// Sets a cached transaction response. The existing key will be overwritten
	/// if it exists.
	pub fn set_cached_txn(&self, key: TxnKey, response: send_transaction_message::v1::Response) {
		let mut state = self.servername_txnid_response_cache.write();
		// TODO: time-to-live?
		state.insert(key, response);
	}
}
