use std::{
	collections::{BTreeMap, HashSet},
	sync::Arc,
	time::Instant,
};

use axum::extract::State;
use conduwuit::{
	Err, Result,
	config::experiments::ToggledExperiment,
	debug_info,
	utils::{ReadyExt, stream::automatic_width},
};
use ruma::{
	MilliSecondsSinceUnixEpoch, OwnedServerName, OwnedServerSigningKeyId,
	api::federation::discovery::{
		ServerSigningKeys, get_remote_server_keys, get_remote_server_keys_batch,
		get_remote_server_keys_batch::v2::QueryCriteria, get_server_keys,
	},
	serde::Raw,
};
use service::server_keys::{self, in_one_week};

use crate::router::Ruma;

/// # `GET /_matrix/key/v2/server`
///
/// Gets the public signing keys of this server.
pub(crate) async fn get_server_keys_route(
	State(services): State<crate::State>,
	_body: Ruma<get_server_keys::v2::Request>,
) -> Result<get_server_keys::v2::Response> {
	services
		.server_keys
		.build_server_keys_response()
		.await
		.map(get_server_keys::v2::Response::new)
}

const MAX_KEYS_PER_QUERY: usize = 16384;
const MAX_SERVERS_PER_QUERY: usize = 4096;

/// # `POST /_matrix/key/v2/query`
///
/// Fetches remote server signing keys in bulk, seeding from the local server's
/// database.
///
/// This route applies a semaphore to slow down requests that query more signing
/// keys than is allowed by [MSC4556].
///
/// [MSC4556]: https://github.com/matrix-org/matrix-spec-proposals/pull/4556
pub(crate) async fn get_remote_server_keys_batch_route(
	State(services): State<crate::State>,
	body: Ruma<get_remote_server_keys_batch::v2::Request>,
) -> Result<get_remote_server_keys_batch::v2::Response> {
	query_inner(&services, &body.server_keys, true)
		.await
		.map(get_remote_server_keys_batch::v2::Response::new)
}

/// # `POST /_matrix/key/unstable/org.continuwuity.msc4556/query`
/// ## `POST /_matrix/key/v3/query`
///
/// Fetches remote server signing keys in bulk, seeding from the local server's
/// database.
///
/// MSC4556: https://github.com/matrix-org/matrix-spec-proposals/pull/4556
pub(crate) async fn get_remote_server_keys_batch_v3_unstable_route(
	State(services): State<crate::State>,
	body: Ruma<ruminuwuity::federation::notary::unstable::Request>,
) -> Result<ruminuwuity::federation::notary::unstable::Response> {
	query_inner(&services, &body.server_keys, false)
		.await
		.map(ruminuwuity::federation::notary::unstable::Response::new)
}

/// Handles querying the server keys for `/_matrix/key/*/query`.
///
/// `v2` should be `true` if the request path is `/_matrix/key/v2/query` - this
/// will potentially apply a rate-limit to the request.
async fn query_inner(
	services: &crate::State,
	body: &BTreeMap<OwnedServerName, BTreeMap<OwnedServerSigningKeyId, QueryCriteria>>,
	v2: bool,
) -> Result<Vec<Raw<ServerSigningKeys>>> {
	let start = (Instant::now(), MilliSecondsSinceUnixEpoch::now());
	// If the remote server isn't asking for any keys, we can return an empty
	// array immediately.
	if body.is_empty() {
		return Ok(Vec::new());
	}

	// Count how many keys are being requested.
	// Each server incurs a cost of at least one, and then the count of each key
	// being queried per-server.
	let total_queried_servers = body.len();
	let total_queried_keys = body
		.values()
		.fold(0_usize, |acc, q| acc.saturating_add(q.len().max(1)));

	if !v2 && services.config.experiments.msc4556.is_enabled() {
		// If we're on the v3 endpoint, we can apply these limits explicitly.
		if total_queried_servers > MAX_SERVERS_PER_QUERY {
			return Err!(Request(TooLarge(
				"Too many server keys requested ({total_queried_servers} > \
				 {MAX_SERVERS_PER_QUERY}",
			)));
		}
		if total_queried_keys > MAX_KEYS_PER_QUERY {
			return Err!(Request(TooLarge(
				"Too many keys requested ({total_queried_keys} > {MAX_KEYS_PER_QUERY})"
			)));
		}
	}
	let penalise =
		total_queried_servers > MAX_SERVERS_PER_QUERY || total_queried_keys > MAX_KEYS_PER_QUERY;

	debug_info!("Fetching {total_queried_keys} keys across {} servers", body.len());
	let mut futs: tokio::task::JoinSet<_> = tokio::task::JoinSet::new();

	// If we're on the v2 endpoint, we aren't allowed to reject the request.
	// To avoid overloading the server, we'll apply a semaphore to limit how
	// many futures can be active at a time.
	let width = automatic_width();

	let semaphore: Option<Arc<tokio::sync::Semaphore>> = if v2 && penalise {
		Some(Arc::new(tokio::sync::Semaphore::new(width)))
	} else {
		None
	};

	for (server_name, queries) in body.clone() {
		let server_keys = services.server_keys.clone();
		let semaphore = semaphore.clone();

		futs.spawn_on(
			async move {
				let _permit = if let Some(sem) = semaphore {
					let cost = queries
						.len()
						.saturating_add(1)
						.min(automatic_width())
						.try_into()
						.unwrap_or(u32::MAX);

					sem.acquire_many_owned(cost).await.ok()
				} else {
					None
				};

				acquire_keys_as_notary(server_keys, server_name, queries, start.1).await
			},
			services.server.runtime(),
		);
	}

	let mut response = Vec::with_capacity(total_queried_keys);

	while let Some(Ok(v)) = futs.join_next().await {
		response.extend(v);
	}

	debug_info!(
		elapsed=?start.0.elapsed(),
		%total_queried_servers,
		%total_queried_keys,
		"Fetched {} key responses",
		response.len()
	);
	Ok(response)
}

/// `GET /_matrix/key/v2/query/{serverName}`
///
/// Fetches the latest keys for a specific server name.
pub(crate) async fn get_remote_server_keys_route(
	State(services): State<crate::State>,
	body: Ruma<get_remote_server_keys::v2::Request>,
) -> Result<get_remote_server_keys::v2::Response> {
	let server_keys = acquire_keys_as_notary(
		services.server_keys.clone(),
		body.server_name.clone(),
		BTreeMap::new(),
		body.minimum_valid_until_ts,
	)
	.await;

	Ok(get_remote_server_keys::v2::Response::new(server_keys))
}

#[tracing::instrument(skip(server_keys, queries, start))]
async fn acquire_keys_as_notary(
	server_keys: Arc<server_keys::Service>,
	remote: OwnedServerName,
	queries: BTreeMap<OwnedServerSigningKeyId, QueryCriteria>,
	start: MilliSecondsSinceUnixEpoch,
) -> Vec<Raw<ServerSigningKeys>> {
	let mut candidate_remote_keys = Vec::with_capacity(queries.len().max(1));

	// If we haven't contacted the origin recently, we need to do that now
	// to update our caches, and we might as well use its response as a
	// candidate.
	if server_keys.origin_not_contacted_recently(&remote) {
		let min_valid_ts = queries
			.values()
			.map(|criterion| criterion.minimum_valid_until_ts.unwrap_or(start))
			.min()
			.unwrap_or(start);

		if let Ok(remote_keys) = server_keys
			.origin_request(remote.clone(), min_valid_ts)
			.await
		{
			debug_info!("Considering keys from origin response");
			candidate_remote_keys.push(remote_keys);
		}
	}

	if queries.is_empty() {
		// The querying server wants all of the origin's keys. If we decided to
		// contact it earlier, we can safely assume its response already
		// includes all of its keys. Otherwise, we'll add all of our cached
		// responses as candidates.

		if candidate_remote_keys.is_empty() {
			debug_info!("Querying server wants all keys, considering all cached key responses");
			server_keys
				.signing_keys_for(&remote)
				.ready_for_each(|remote_keys| candidate_remote_keys.push(remote_keys))
				.await;
		}
	} else {
		// The querying server is asking for specific key IDs.

		for key_id in queries.keys() {
			if let Some(remote_keys) = server_keys.get_signing_key(&remote, key_id).await {
				debug_info!(?key_id, "Considering cached response containing key");

				candidate_remote_keys.push(remote_keys);
			} else {
				debug_info!(?key_id, "No cached response contains key");
			}
		}
	}

	// Now we need to filter and sign the remote key responses.

	// The signing key responses containing the keys the querying server wants.
	let mut signed_remote_key_responses = Vec::with_capacity(queries.len().max(1));

	// The key IDs we've already included in a response.
	let mut signed_remote_key_ids = HashSet::with_capacity(queries.len().max(1));

	for remote_keys in candidate_remote_keys {
		// Skip this key response if it expires too far in the future.
		if remote_keys.parsed.valid_until_ts > in_one_week() {
			continue;
		}

		// Determine which requested keys (or all keys when no specific keys
		// were requested) are newly satisfied by this response.
		let newly_satisfied: Vec<OwnedServerSigningKeyId> = if queries.is_empty() {
			if remote_keys.parsed.valid_until_ts < start {
				continue;
			}

			remote_keys
				.all_key_ids()
				.filter(|key_id| !signed_remote_key_ids.contains(*key_id))
				.map(ToOwned::to_owned)
				.collect()
		} else {
			queries
				.iter()
				.filter_map(|(key_id, criterion)| {
					if signed_remote_key_ids.contains(key_id) {
						return None;
					}

					let minimum_valid_until_ts =
						criterion.minimum_valid_until_ts.unwrap_or(start);

					if remote_keys.parsed.valid_until_ts < minimum_valid_until_ts {
						return None;
					}

					let satisfied = remote_keys.parsed.verify_keys.contains_key(key_id)
						|| remote_keys.parsed.old_verify_keys.get(key_id).is_some_and(
							|old_key| {
								criterion
									.minimum_valid_until_ts
									.is_none_or(|min_ts| old_key.expired_ts >= min_ts)
							},
						);

					satisfied.then(|| key_id.to_owned())
				})
				.collect()
		};

		if newly_satisfied.is_empty() {
			continue;
		}

		// This response is acceptable, let's sign and send it.

		let signed_remote_keys = server_keys
			.self_sign_signing_keys(&remote, &remote_keys.raw)
			.expect("should be able to sign remote keys");

		signed_remote_key_responses.push(signed_remote_keys);
		signed_remote_key_ids.extend(newly_satisfied);
	}

	signed_remote_key_responses
}
