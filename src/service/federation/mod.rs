mod execute;

use std::{collections::HashMap, sync::Arc};

use conduwuit::{Err, Result, Server, SyncRwLock, err};
pub(crate) use execute::FederationPathBuilderInput;

use crate::{Dep, client, moderation, server_keys};

pub struct Service {
	services: Services,
	/// A map of {answer: channel}
	pingpongs: SyncRwLock<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
}

struct Services {
	server: Arc<Server>,
	client: Dep<client::Service>,
	server_keys: Dep<server_keys::Service>,
	moderation: Dep<moderation::Service>,
}

impl crate::Service for Service {
	fn build(args: crate::Args<'_>) -> Result<Arc<Self>> {
		Ok(Arc::new(Self {
			services: Services {
				server: args.server.clone(),
				client: args.depend::<client::Service>("client"),
				server_keys: args.depend::<server_keys::Service>("server_keys"),
				moderation: args.depend::<moderation::Service>("moderation"),
			},
			pingpongs: SyncRwLock::new(HashMap::new()),
		}))
	}

	fn name(&self) -> &str { crate::service::make_name(std::module_path!()) }
}

impl Service {
	/// Registers an outbound ping waiter based on the answer returned by the
	/// remote. Returns a channel that is written to when the remote pongs.
	pub fn register_ping_answer(
		&self,
		expected_answer: String,
	) -> Result<tokio::sync::oneshot::Receiver<()>> {
		let mut pingpongs = self.pingpongs.write();
		if pingpongs.contains_key(&expected_answer) {
			return Err!(Request(InvalidParam("Duplicate answer")));
		}
		let (tx, rx) = tokio::sync::oneshot::channel();
		pingpongs.insert(expected_answer, tx);
		Ok(rx)
	}

	/// "Answers" a registered outbound ping by sending an event to it. This is
	/// called when the remote server that was pinged calls /pong.
	///
	/// `M_NOT_FOUND` is returned if the answer is not recognised.
	pub fn answer_ping(&self, answer: &str) -> Result<()> {
		let mut pingpongs = self.pingpongs.write();
		let Some(tx) = pingpongs.remove(answer) else {
			return Err!(Request(NotFound("Unknown answer")));
		};
		tx.send(()).map_err(|e| {
			err!(BadServerResponse(error!(
				error=?e, "Failed to handle pong"
			)))
		})
	}
}
