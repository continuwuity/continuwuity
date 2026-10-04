/// get.rs handles actually acquiring keys from remote servers.
use std::collections::BTreeMap;

use assign::assign;
use continuwuity::{Result, debug, debug_error, debug_info, debug_warn, err, info, trace};
use futures::{StreamExt, stream::FuturesUnordered};
use ruma::{
	CanonicalJsonObject, CanonicalJsonValue, MilliSecondsSinceUnixEpoch, ServerName,
	ServerSigningKeyId, UInt,
	api::federation::discovery::{
		ServerSigningKeys, VerifyKey, get_remote_server_keys_batch::v2::QueryCriteria,
	},
	room_version_rules::RoomVersionRules,
	serde::Base64,
	uint,
};
use serde_json::value::RawValue;
use tokio::select;

use super::{PubKeyMap, in_one_week};
use crate::server_keys::{request::ServerKeysQuery, util::required_keys};

impl super::Service {
	/// Extracts the minimum timestamp within which a signing key must be valid
	/// for it to be able to verify this event.
	pub(super) fn min_valid_ts_from_event(
		object: &CanonicalJsonObject,
	) -> Result<MilliSecondsSinceUnixEpoch> {
		object
			.get("origin_server_ts")
			.and_then(CanonicalJsonValue::as_integer)
			.map(i64::from)
			.map(|ts| {
				MilliSecondsSinceUnixEpoch(UInt::new_saturating(
					u64::try_from(ts).unwrap_or_default(),
				))
			})
			.ok_or_else(|| err!(Request(BadJson("Missing origin_server_ts in event"))))
	}

	/// Fetches the verify keys required to verify the incoming event.
	///
	/// Only returns an error if the `min_valid_ts` cannot be inferred from the
	/// object, or if calculating which keys are required fails (malformed
	/// event).
	///
	/// Keys which cannot be fetched are not included in the resulting map, but
	/// will not cause an error.
	pub async fn get_event_keys(
		&self,
		object: &CanonicalJsonObject,
		room_version_rules: &RoomVersionRules,
	) -> Result<PubKeyMap> {
		let min_valid_ts = Self::min_valid_ts_from_event(object)?;

		let required = required_keys(object, &room_version_rules.signatures).map_err(|e| {
			err!(BadServerResponse("Failed to determine keys required to verify event: {e}"))
		})?;

		let batch = required
			.into_iter()
			.map(|(s, ids)| {
				(
					s,
					ids.into_iter()
						.map(|key_id| (key_id, new_query_criteria(min_valid_ts)))
						.collect::<BTreeMap<_, _>>(),
				)
			})
			.collect::<ServerKeysQuery>();

		Ok(self.fetch_verify_keys(batch).await)
	}

	/// Fetches the verify keys required to verify all the incoming events.
	pub async fn get_events_keys(
		&self,
		raw_objects: impl Iterator<Item = &Box<RawValue>>,
		room_version_rules: &RoomVersionRules,
	) -> PubKeyMap {
		let mut batch = ServerKeysQuery::new();
		for raw_object in raw_objects {
			let object: CanonicalJsonObject = match serde_json::from_str(raw_object.get()) {
				| Ok(object) => object,
				| Err(e) => {
					debug_error!(?raw_object, error=?e, "failed to parse incoming event");
					continue;
				},
			};
			let min_valid_ts = match Self::min_valid_ts_from_event(&object) {
				| Ok(ts) => ts,
				| Err(e) => {
					debug!(?object, error=?e, "incoming event has no `origin_server_ts` field, skipping");
					continue;
				},
			};
			let required = match required_keys(&object, &room_version_rules.signatures) {
				| Ok(required) => required,
				| Err(e) => {
					debug_warn!(
						?object,
						error=?e,
						"failed to determine required verify keys for incoming event, skipping"
					);
					continue;
				},
			};
			for (server_name, signing_key_ids) in required {
				let batch_entry = batch.entry(server_name).or_default();
				for key_id in signing_key_ids {
					batch_entry
						.entry(key_id)
						.and_modify(|criteria| {
							// We want to use the lower of the two minimum valid
							// timestamps if the key is already existing;
							// otherwise, just insert this one.
							criteria.minimum_valid_until_ts = Some(
								criteria
									.minimum_valid_until_ts
									.expect("We never ask for no min valid ts")
									.min(min_valid_ts),
							);
						})
						.or_insert_with(|| new_query_criteria(min_valid_ts));
				}
			}
		}

		self.fetch_verify_keys(batch).await
	}

	/// Fetches verify keys matching the required batch parameters.
	///
	/// First attempts to fulfil any requirements from the local server cache,
	/// falling back to asking origins and notaries.
	///
	/// Origins and notaries are always asked in parallel, and will complement
	/// each other if notaries are not prioritised, using a
	/// first-come-first-serve stream consumption approach.
	///
	/// If notaries are prioritised, this function will instead drain all notary
	/// futures until all keys are found, or there are no more notary responses.
	/// In that case, the origin future(s) will be consumed.
	///
	/// If there are still unacquired keys, a debug info log will be emitted
	/// containing further details, but an error will not be returned.
	async fn fetch_verify_keys(&self, batch: ServerKeysQuery) -> PubKeyMap {
		let mut still_missing = batch.clone();
		let mut result = PubKeyMap::new();
		trace!(?batch, "Preparing to fetch verify keys");

		// First, filter out any keys we already have locally.
		for (origin, keys) in &batch {
			for (key_id, criteria) in keys {
				let min_valid_ts = criteria
					.minimum_valid_until_ts
					.unwrap_or_else(|| MilliSecondsSinceUnixEpoch(uint!(0)));

				if let Some(server_keys) = self.get_signing_key(origin, key_id).await
					&& let Some(key) =
						verify_key_from_response(key_id, &server_keys, min_valid_ts)
				{
					trace!(%origin, %key_id, ?min_valid_ts, "Verify key exists locally");
					Self::track_one_result(
						&mut result,
						&mut still_missing,
						origin,
						key_id,
						key.key,
					);
					continue;
				}
				trace!(%origin, %key_id, ?min_valid_ts, "Did not find verify key locally");
			}
		}
		if still_missing.is_empty() {
			trace!("Not missing any more keys!");
			return result;
		}
		trace!(?still_missing, "Still missing some keys after local population");

		// Now we need to fetch keys en masse.
		// For each server in the set, we'll ping off a request to the origin to
		// fetch their signing keys. Then, in parallel, we'll ask every notary
		// we know for the full batch. If the server is configured to ask the
		// origin first, we'll simply await the origin request first, failing
		// back to the notary futures. Otherwise, we can work through each
		// response as they come back until we either get all the keys we
		// need, or we run out of servers to ask.
		let mut origin_futures: FuturesUnordered<_> = FuturesUnordered::new();
		let mut notary_futures: FuturesUnordered<_> = FuturesUnordered::new();

		for origin in still_missing.keys() {
			let min_valid_ts_for_this_remote = batch
				.get(origin)
				.and_then(|keys| {
					keys.values()
						.filter_map(|criteria| criteria.minimum_valid_until_ts)
						.max()
				})
				.unwrap_or_else(|| MilliSecondsSinceUnixEpoch(uint!(0)));
			trace!(
				%origin,
				?min_valid_ts_for_this_remote,
				"Spawning future to ask origin for their verify keys"
			);
			origin_futures
				.push(self.origin_request(origin.to_owned(), min_valid_ts_for_this_remote));
		}

		self.services
			.server
			.config
			.trusted_servers
			.iter()
			.for_each(|notary| {
				trace!(notary=%notary.server_name(), "Asking notary for still missing keys");
				notary_futures
					.push(self.notary_request(notary.server_name(), still_missing.clone()));
			});

		let prioritise_notaries = !self.services.server.config.trusted_servers.is_empty()
			&& self.services.server.config.query_trusted_key_servers_first;

		// TODO: In the future, an option to establish a quorum between notaries
		// may be desired. In such a case, the `still_missing { break }`
		// blocks should be skipped in order to acquire all notary responses for
		// later comparison.

		if !prioritise_notaries {
			// If we aren't prioritising notaries, we can just select futures on
			// a first-come-first-serve basis.
			loop {
				select! {
					origin_response = origin_futures.next(), if !origin_futures.is_empty() => {
						let origin_response = match origin_response {
							Some(Ok(v)) => v,
							None => continue,
							Some(Err(e)) => {
								debug_warn!(err=?e, "Got error response from origin");
								continue;
							}
						};
						trace!(?origin_response, "Got signing key response from origin");
						self.add_signing_keys(&origin_response);
						Self::track_result(&mut result, &mut still_missing, &origin_response);
						if still_missing.is_empty() {
							break;
						}
					},
					notary_response = notary_futures.next(), if !notary_futures.is_empty() => {
						let notary_response = match notary_response {
							Some(Ok(v)) => v,
							None => continue,
							Some(Err(e)) => {
								debug_warn!(err=?e, "Got error response from notary");
								continue;
							}
						};
						trace!(
							chunks=%notary_response.len(),
							"Got signing keys response from a notary"
						);
						for server_keys in notary_response {
							trace!(?server_keys, "Adding response from notary");
							self.add_signing_keys(&server_keys);
							Self::track_result(&mut result, &mut still_missing, &server_keys);
							if still_missing.is_empty() {
								break;
							}
						}
					}
					else => {
						trace!("Origin or notary future stream(s) finished.");
						break
					}
				}
			}
		} else {
			// Drain notary responses first, and then sift through origins.
			trace!(count=%notary_futures.len(), "Waiting for notary responses...");
			while let Some(res) = notary_futures.next().await {
				let Ok(res) = res else {
					debug_warn!(err=?res.err(), "Got error response from notary");
					continue;
				};
				for server_keys in res {
					trace!(?server_keys, "Adding response from notary");
					self.add_signing_keys(&server_keys);
					Self::track_result(&mut result, &mut still_missing, &server_keys);
				}
				if still_missing.is_empty() {
					break;
				}
			}
			if !still_missing.is_empty() {
				trace!("Still missing some keys, waiting for origin responses...");
				while let Some(res) = origin_futures.next().await {
					let Ok(res) = res else {
						debug_warn!(err=?res.err(), "Got error response from origin");
						continue;
					};
					trace!(response=?res, "Got signing key response from origin");
					self.add_signing_keys(&res);
					Self::track_result(&mut result, &mut still_missing, &res);
					if still_missing.is_empty() {
						break;
					}
				}
			}
		}

		if !still_missing.is_empty() {
			let missing_servers = still_missing.len();
			let missing_keys = still_missing
				.values()
				.map(BTreeMap::len)
				.fold(0_usize, usize::saturating_add);
			let fetched_servers = result.len();
			let fetched_keys = result
				.values()
				.map(BTreeMap::len)
				.fold(0_usize, usize::saturating_add);
			debug_info!(
				%fetched_servers,
				%fetched_keys,
				%missing_servers,
				%missing_keys,
				"Could not acquire all requested signing keys"
			);
			trace!(?still_missing, "Could not acquire all requested signing keys");
		}
		result
	}

	/// Tracks the result of a signing keys response by removing each of the
	/// returned verify keys and old verify keys from the missing map, and
	/// adding them to the results map.
	fn track_result(
		results: &mut PubKeyMap,
		missing: &mut ServerKeysQuery,
		batch: &ServerSigningKeys,
	) {
		batch.verify_keys.iter().for_each(|(key_id, key)| {
			Self::track_one_result(results, missing, &batch.server_name, key_id, key.key.clone());
		});
		batch.old_verify_keys.iter().for_each(|(key_id, key)| {
			Self::track_one_result(results, missing, &batch.server_name, key_id, key.key.clone());
		});
	}

	/// Tracks a single signing key result by adding it to the results map and
	/// removing it from the missing map. If all missing keys for an origin are
	/// found (the missing list is empty), the origin is removed from the
	/// missing map entirely.
	fn track_one_result(
		results: &mut PubKeyMap,
		missing: &mut ServerKeysQuery,
		origin: &ServerName,
		key_id: &ServerSigningKeyId,
		key: Base64,
	) {
		results
			.entry(origin.to_string())
			.or_default()
			.insert(key_id.to_string(), key);
		missing.entry(origin.to_owned()).and_modify(|keys| {
			keys.remove(key_id);
		});
		if missing.get(origin).is_some_and(BTreeMap::is_empty) {
			missing.remove(origin);
		}
	}

	/// Fetches a single verify key.
	///
	/// If a valid key cannot be found locally, the origin is queried. If it
	/// fails to respond, or the desired verify key is not contained in the
	/// response/expired before `min_valid_ts`, `None` is returned.
	#[tracing::instrument(skip(self))]
	pub async fn get_single_verify_key(
		&self,
		origin: &ServerName,
		key_id: &ServerSigningKeyId,
		min_valid_ts: MilliSecondsSinceUnixEpoch,
	) -> Option<VerifyKey> {
		if let Some(vk) = self
			.get_signing_key(origin, key_id)
			.await
			.and_then(|ssk| verify_key_from_response(key_id, &ssk, min_valid_ts))
		{
			trace!("Found verify key locally");
			return Some(vk);
		}

		trace!("Local miss for single key, asking origin");
		let response = self
			.origin_request(origin.to_owned(), min_valid_ts)
			.await
			.inspect_err(|e| info!("Could not acquire signing key from origin server: {e:?}"))
			.ok()?;

		verify_key_from_response(key_id, &response, min_valid_ts)
			.inspect(|_| self.add_signing_keys(&response))
	}
}

/// Generates a QueryCriteria with the minimum_valid_until_ts set to the target
/// epoch.
fn new_query_criteria(min_valid_ts: MilliSecondsSinceUnixEpoch) -> QueryCriteria {
	assign!(QueryCriteria::new(), { minimum_valid_until_ts: Some(min_valid_ts) })
}

/// Extracts a verify key with the given ID from the response.
///
/// If the provided `ServerSigningKeys` is dated to before the min valid TS,
/// this will throw an assertion panic in debug builds, or always return `None`
/// in release. If the key is found in `verify_keys`, it is returned as-is. If
/// the key is found in `old_verify_keys`, it is only returned if the expiry
/// timestamp is greater than or equal to the minimum valid timestamp.
fn verify_key_from_response(
	key_id: &ServerSigningKeyId,
	server_keys: &ServerSigningKeys,
	min_valid_ts: MilliSecondsSinceUnixEpoch,
) -> Option<VerifyKey> {
	if server_keys.valid_until_ts < min_valid_ts || server_keys.valid_until_ts > in_one_week() {
		return None;
	}
	if let Some(key) = server_keys
		.verify_keys
		.iter()
		.filter(|(k, _)| *k == key_id)
		.map(|(_, v)| v)
		.next()
	{
		return Some(key.to_owned());
	}
	server_keys
		.old_verify_keys
		.iter()
		.filter(|(k, v)| {
			*k == key_id && v.expired_ts.min(server_keys.valid_until_ts) >= min_valid_ts
		})
		.map(|(_, v)| v)
		.next()
		.map(|old_key| VerifyKey::new(old_key.key.clone()))
}

#[cfg(test)]
mod test {
	use ruma::Int;
	use serde_json::json;

	use super::*;
	use crate::server_keys::{Service, request::ServerKeysQuerySet};

	#[test]
	fn min_valid_ts_refuses_garbage() {
		let garbage = vec![
			json!("string"),
			json!("0"), // string ints aren't ints
			CanonicalJsonValue::Null.into(),
			json!([]),
			json!({}),
			json!(" "),
		];

		for fixture in garbage {
			let obj: CanonicalJsonObject =
				serde_json::from_value(json!({"origin_server_ts": fixture})).unwrap();
			assert!(
				Service::min_valid_ts_from_event(&obj).is_err(),
				"garbage should always error instead of trying to guess"
			);
		}
	}

	#[test]
	fn min_valid_ts_requires_field() {
		// This just ensures we don't accidentally default to 0 or something
		// dumb like that
		assert!(
			Service::min_valid_ts_from_event(&CanonicalJsonObject::new()).is_err(),
			"origin_server_ts should be a required field and thus should error if not provided"
		);
	}

	#[test]
	fn min_valid_ts_accepts_negative_values() {
		// Since we convert to u64 with saturating for MSSinceEpoch, this should
		// give us a final timestamp of zero (since unsigned ints cant be
		// negative).
		let obj: CanonicalJsonObject =
			serde_json::from_value(json!({"origin_server_ts": Int::MIN})).unwrap();
		assert_eq!(
			Service::min_valid_ts_from_event(&obj)
				.expect("Int::MIN should be a valid min timestamp")
				.0,
			uint!(0),
			"Saturating Int::MIN to u64 should result in zero"
		);
	}

	#[test]
	fn tracking_server_result_removes_empty_server_from_missing_map() {
		let server_name = ruma::owned_server_name!("example.com");
		let key_id = ServerSigningKeyId::parse("ed25519:1").unwrap();
		let mut missing = ServerKeysQuery::new();
		missing.insert(server_name.clone(), ServerKeysQuerySet::new());
		missing.entry(server_name.clone()).and_modify(|set| {
			set.insert(key_id.clone(), QueryCriteria::new());
		});
		let mut results = PubKeyMap::new();

		Service::track_one_result(
			&mut results,
			&mut missing,
			&server_name,
			&key_id,
			Base64::new(Vec::new()),
		);

		assert!(missing.is_empty(), "missing should be empty after tracking the result");
		assert!(!results.is_empty(), "results should not be empty after tracking the result");
	}
}
