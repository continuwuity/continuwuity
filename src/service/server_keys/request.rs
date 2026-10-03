use std::{collections::BTreeMap, time::Instant};

use conduwuit::{Err, Result, trace, utils::millis_since_unix_epoch, warn};
use ruma::{
	MilliSecondsSinceUnixEpoch, OwnedServerName, OwnedServerSigningKeyId, ServerName, UInt,
	api::federation::discovery::{
		ServerSigningKeys,
		get_remote_server_keys_batch::{
			self,
			v2::{QueryCriteria, Response as NotaryResponse},
		},
		get_server_keys,
	},
	serde::Base64,
	uint,
};

pub(crate) type ServerKeysQuery = BTreeMap<OwnedServerName, ServerKeysQuerySet>;
pub(crate) type ServerKeysQuerySet = BTreeMap<OwnedServerSigningKeyId, QueryCriteria>;

impl super::Service {
	/// Asks the origin server directly for its signing keys.
	///
	/// An error is returned if the server responds with invalid keys; this is
	/// in contrast with the notary request, which simply drops such responses.
	pub async fn origin_request(
		&self,
		target: OwnedServerName,
		min_valid_ts: MilliSecondsSinceUnixEpoch,
	) -> Result<ServerSigningKeys> {
		use get_server_keys::v2::Request;

		// N.B. The "last lookup" is written before the request is actually made
		// to prevent concurrent notary requests from spawning... concurrent
		// origin requests, especially if the origin is slow.
		// This has the downside that, if the origin is temporarily unreachable
		// (including if we're backing off from it), the notary might take an
		// additional minute to recover compared to the rest of the server. This
		// is deemed acceptable.
		self.last_lookup
			.write()
			.insert(target.clone(), Instant::now());
		let server_signing_key = self
			.services
			.sending
			.send_unauthenticated_request(&target, Request::new())
			.await
			.map(|response| response.server_key)
			.and_then(|key| key.deserialize().map_err(Into::into))?;

		if server_signing_key.server_name != target {
			return Err!(BadServerResponse(debug_warn!(
				requested = ?target,
				response = ?server_signing_key.server_name,
				"Origin server responded with bogus server_name"
			)));
		}

		if server_signing_key.valid_until_ts < min_valid_ts
			|| server_signing_key.valid_until_ts > in_one_week()
		{
			return Err!(BadServerResponse(debug_warn!(
				?target,
				?min_valid_ts,
				?server_signing_key.valid_until_ts,
				"Origin server responded with server keys that are not in-date"
			)));
		}

		if let Err(e) = Self::verify_server_keys_response(&server_signing_key, None) {
			return Err!(BadServerResponse(debug_warn!(
				?target,
				"Origin server failed to self-sign key response: {e}"
			)));
		}

		Ok(server_signing_key)
	}

	/// Sends a request to a notary server, asking for the specified keys for
	/// the given servers.
	///
	/// The response is filtered before it is returned, dropping any chunks that
	/// are invalid or do not match the requested keys. As a result, the request
	/// may be successful, but the returned vec may be empty.
	///
	///
	/// An error is returned if the notary server itself returns an error,
	/// because of invalid data.
	///
	/// Does not make a request if there are no keys specified in the batch.
	pub async fn notary_request(
		&self,
		notary: &ServerName,
		batch: ServerKeysQuery,
	) -> Result<Vec<ServerSigningKeys>> {
		use get_remote_server_keys_batch::v2::Request;

		let is_empty = batch.is_empty() || batch.values().all(BTreeMap::is_empty);
		if is_empty {
			return Ok(Vec::new());
		}

		let request = Request::new(batch.clone());
		let notary_keys = self
			.services
			.server
			.config
			.trusted_servers
			.iter()
			.find(|n| n.server_name() == notary)
			.map(|nk| nk.verify_keys().to_owned())
			.unwrap_or_default();
		let keys = self
			.services
			.sending
			.send_unauthenticated_request(notary, request)
			.await
			.map(|response| Self::parse_notary_response(&response))?
			.into_iter()
			.filter(|keys| {
				Self::validate_notary_chunk(&batch, keys, notary, notary_keys.as_slice())
			})
			.collect();
		Ok(keys)
	}

	/// Parses a notary response, returning a vec of server signing key
	/// responses. Drops any chunks that cannot be deserialized.
	fn parse_notary_response(raw: &NotaryResponse) -> Vec<ServerSigningKeys> {
		raw.server_keys
			.iter()
			.map(ruma::serde::Raw::deserialize)
			.filter_map(Result::ok)
			.collect()
	}

	/// Validates a signing keys response received from a notary. Returns false
	/// if the response should be dropped, true if it should be kept.
	fn validate_notary_chunk(
		batch: &ServerKeysQuery,
		keys: &ServerSigningKeys,
		notary_name: &ServerName,
		notary_keys: &[Base64],
	) -> bool {
		let Some(expected_keys) = batch.get(&keys.server_name) else {
			trace!("Dropping signing keys chunk for unrequested server: {}", keys.server_name);
			return false;
		};
		let min_valid_ts = expected_keys
			.values()
			.map(|q| {
				q.minimum_valid_until_ts
					.unwrap_or_else(|| MilliSecondsSinceUnixEpoch(uint!(0)))
			})
			.min()
			.unwrap_or_else(|| MilliSecondsSinceUnixEpoch(uint!(0)));

		// If this notary response is older than any of the min_valid_ts we
		// requested, then we should drop it.
		if keys.valid_until_ts < min_valid_ts || keys.valid_until_ts > in_one_week() {
			trace!(
				?min_valid_ts,
				?keys.valid_until_ts,
				"Dropping signing keys chunk for server {} because it is older than the minimum \
				 valid timestamp we requested",
				keys.server_name
			);
			return false;
		}

		// Does this chunk even contain any keys we asked for?
		// Note that this enforces min_valid_ts per-key, instead of using the
		// lowest denominator.
		let all_key_ids = keys
			.verify_keys
			.keys()
			.chain(keys.old_verify_keys.keys())
			.map(ToOwned::to_owned)
			.collect::<Vec<_>>();
		let interesting = expected_keys.iter().any(|(k, q)| {
			all_key_ids.contains(k)
				&& q.minimum_valid_until_ts
					.is_none_or(|m| m <= keys.valid_until_ts)
		});
		if !interesting {
			trace!(
				"Dropping signing keys chunk for server {} because it does not contain any keys \
				 we requested",
				keys.server_name
			);
			return false;
		}

		Self::verify_server_keys_response(keys, Some((notary_name, notary_keys)))
			.inspect_err(|e| {
				trace!(
					?e,
					?keys,
					?notary_name,
					?notary_keys,
					"Dropping illegal server signing keys response chunk"
				);
				warn!(
					%notary_name,
					server_name=%keys.server_name,
					"Dropping illegal server signing keys response chunk: {e:?}"
				);
			})
			.is_ok()
	}
}

#[must_use]
pub fn in_one_week() -> MilliSecondsSinceUnixEpoch {
	MilliSecondsSinceUnixEpoch(
		UInt::new_saturating(millis_since_unix_epoch()).saturating_add(uint!(604_800_000)),
	)
}

// TODO(nex): Unit tests for validate_notary_chunk would be really good, but are
// hard to do by nature of time advancing, signatures, and external notaries.
// We can rely on integration tests for now.
