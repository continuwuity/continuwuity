use std::{collections::BTreeMap, fmt::Debug};

use conduwuit::{Err, Result, debug, implement};
use ruma::{
	OwnedServerName, OwnedServerSigningKeyId, ServerName, ServerSigningKeyId,
	api::federation::discovery::{
		ServerSigningKeys, get_remote_server_keys,
		get_remote_server_keys_batch::{self, v2::QueryCriteria},
		get_server_keys,
	},
};

#[implement(super::Service)]
pub(super) async fn batch_notary_request<'a, S, K>(
	&self,
	notary: &ServerName,
	batch: S,
) -> Result<Vec<ServerSigningKeys>>
where
	S: Iterator<Item = (&'a ServerName, K)> + Send,
	K: Iterator<Item = &'a ServerSigningKeyId> + Send,
{
	use get_remote_server_keys_batch::v2::Request;
	type RumaBatch = BTreeMap<OwnedServerName, BTreeMap<OwnedServerSigningKeyId, QueryCriteria>>;

	let mut criteria = QueryCriteria::new();
	criteria.minimum_valid_until_ts = Some(self.minimum_valid_ts());

	let mut server_keys = batch.fold(RumaBatch::new(), |mut batch, (server, key_ids)| {
		batch
			.entry(server.into())
			.or_default()
			.extend(key_ids.map(|key_id| (key_id.into(), criteria.clone())));

		batch
	});

	debug_assert!(!server_keys.is_empty(), "empty batch request to notary");

	let mut results = Vec::new();
	while let Some(batch) = server_keys
		.keys()
		.rev()
		.take(self.services.server.config.trusted_server_batch_size)
		.next_back()
		.cloned()
	{
		let request = Request::new(server_keys.split_off(&batch));

		debug!(
			?notary,
			?batch,
			remaining = %server_keys.len(),
			requesting = ?request.server_keys.keys(),
			"notary request"
		);

		let response = self
			.services
			.sending
			.send_unauthenticated_request(notary, request)
			.await?
			.server_keys
			.into_iter()
			.map(|key| key.deserialize())
			.filter_map(Result::ok);

		results.extend(response);
	}

	Ok(results)
}

#[implement(super::Service)]
pub async fn notary_request(
	&self,
	notary: &ServerName,
	target: &ServerName,
) -> Result<impl Iterator<Item = ServerSigningKeys> + Clone + Debug + Send + use<>> {
	use get_remote_server_keys::v2::Request;

	let request = Request::new(target.into(), self.minimum_valid_ts());

	let response = self
		.services
		.sending
		.send_unauthenticated_request(notary, request)
		.await?
		.server_keys
		.into_iter()
		.map(|key| key.deserialize())
		.filter_map(Result::ok);

	Ok(response)
}

#[implement(super::Service)]
pub async fn server_request(&self, target: &ServerName) -> Result<ServerSigningKeys> {
	use get_server_keys::v2::Request;

	let server_signing_key = self
		.services
		.sending
		.send_unauthenticated_request(target, Request::new())
		.await
		.map(|response| response.server_key)
		.and_then(|key| key.deserialize().map_err(Into::into))?;

	if server_signing_key.server_name != target {
		return Err!(BadServerResponse(debug_warn!(
			requested = ?target,
			response = ?server_signing_key.server_name,
			"Server responded with bogus server_name"
		)));
	}

	Ok(server_signing_key)
}
