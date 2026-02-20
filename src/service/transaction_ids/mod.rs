use std::{collections::HashMap, sync::Arc};

use conduwuit::{Result, SyncRwLock};
use database::{Handle, Map};
use ruma::{
	DeviceId, OwnedServerName, OwnedTransactionId, TransactionId, UserId,
	api::federation::transactions::send_transaction_message,
};
use tokio::sync::watch::{Receiver, Sender};

pub type TxnKey = (OwnedServerName, OwnedTransactionId);
pub type TxnChanType = (TxnKey, send_transaction_message::v1::Response);
pub type ActiveTxnsMapType = HashMap<TxnKey, (Sender<TxnChanType>, Receiver<TxnChanType>)>;

pub struct Service {
	db: Data,
	pub servername_txnid_response_cache:
		Arc<SyncRwLock<HashMap<TxnKey, send_transaction_message::v1::Response>>>,
	pub servername_txnid_active: Arc<SyncRwLock<ActiveTxnsMapType>>,
}

struct Data {
	userdevicetxnid_response: Arc<Map>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			db: Data {
				userdevicetxnid_response: args.db["userdevicetxnid_response"].clone(),
			},
			servername_txnid_response_cache: Arc::new(SyncRwLock::new(HashMap::new())),
			servername_txnid_active: Arc::new(SyncRwLock::new(HashMap::new())),
		}))
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
}
