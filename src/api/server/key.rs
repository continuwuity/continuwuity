use std::{
	collections::{BTreeMap, HashMap, HashSet},
	ops::Index,
	sync::Arc,
	time::Instant,
};

use axum::extract::State;
use conduwuit::{
	Err, Result, debug, debug_info, error,
	utils::{ReadyExt, stream::BroadbandExt, to_canonical_object},
	warn,
};
use futures::{StreamExt, stream::FuturesUnordered};
use ruma::{
	MilliSecondsSinceUnixEpoch, OwnedServerName, OwnedServerSigningKeyId, ServerName,
	api::federation::discovery::{
		ServerSigningKeys, get_remote_server_keys, get_remote_server_keys_batch,
		get_remote_server_keys_batch::v2::QueryCriteria, get_server_keys,
	},
	serde::Raw,
	uint,
};
use serde_json::value::to_raw_value;
use service::{server_keys, server_keys::in_one_week};

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

pub(crate) async fn get_remote_server_keys_batch_route(
	State(services): State<crate::State>,
	body: Ruma<get_remote_server_keys_batch::v2::Request>,
) -> Result<get_remote_server_keys_batch::v2::Response> {
	let start = (Instant::now(), MilliSecondsSinceUnixEpoch::now());
	if body.server_keys.is_empty() {
		return Ok(get_remote_server_keys_batch::v2::Response::new(Vec::new()));
	}

	let total_queried_servers = body.server_keys.len();
	let total_queried_keys = body
		.server_keys
		.values()
		.fold(body.server_keys.len(), |acc, q| acc.saturating_add(q.len()));

	if body.server_keys.len() > MAX_SERVERS_PER_QUERY {
		// TODO(nex): enforce once MSC4556 is merged
		warn!(
			%total_queried_servers,
			%total_queried_keys,
			"Received a large notary request (too many servers)"
		);
		// return Err!(Request(TooLarge(
		// 	"Too many server keys requested ({} > {MAX_SERVERS_PER_QUERY}",
		// 	body.server_keys.len()
		// )));
	}
	if total_queried_keys > MAX_KEYS_PER_QUERY {
		// We shouldn't really enforce this before 4556 either, but not doing
		// so may cause performance degradation.
		warn!(
			%total_queried_servers,
			%total_queried_keys,
			"Received a huge notary request (too many keys), rejecting",
		);
		return Err!(Request(TooLarge(
			"Too many keys requested ({total_queried_keys} > {MAX_KEYS_PER_QUERY})"
		)));
	}

	debug_info!("Fetching {total_queried_keys} keys across {} servers", body.server_keys.len());
	let mut futs: FuturesUnordered<_> = FuturesUnordered::new();
	for (server_name, queries) in body.server_keys.clone() {
		futs.push(acquire_keys_as_notary(
			services.server_keys.clone(),
			services.globals.server_name().to_owned(),
			server_name,
			queries,
			start.1,
		));
	}
	let mut response = Vec::with_capacity(total_queried_keys);
	while let Some(v) = futs.next().await {
		response.extend(v);
	}

	debug_info!(
		elapsed=?start.0.elapsed(),
		%total_queried_servers,
		%total_queried_keys,
		"Fetched {} key responses",
		response.len()
	);
	Ok(get_remote_server_keys_batch::v2::Response::new(response))
}

async fn sign_ssk(
	server_keys: &server_keys::Service,
	ssk: ServerSigningKeys,
	server_name: &ServerName,
	our_name: &ServerName,
) -> Result<Raw<ServerSigningKeys>> {
	if server_name == our_name {
		// Should already be signed by us if we got this far.
		debug_info!(
			object=?ssk,
			%server_name,
			%our_name,
			"Refusing to sign our own signing keys response (should already be signed)"
		);
		return to_raw_value(&ssk)
			.map(Raw::<ServerSigningKeys>::from_json)
			.map_err(Into::into);
	}
	let mut canonical = to_canonical_object(&ssk)?;
	server_keys.sign_json(&mut canonical)?;
	server_keys::strip_extraneous_signatures(&mut canonical, server_name, &[our_name]);
	to_raw_value(&canonical)
		.map(Raw::<ServerSigningKeys>::from_json)
		.map_err(Into::into)
}

#[tracing::instrument(skip(server_keys, my_name, queries, start))]
async fn acquire_keys_as_notary(
	server_keys: Arc<server_keys::Service>,
	my_name: OwnedServerName,
	remote: OwnedServerName,
	mut queries: BTreeMap<OwnedServerSigningKeyId, QueryCriteria>,
	start: MilliSecondsSinceUnixEpoch,
) -> Vec<Raw<ServerSigningKeys>> {
	let fetch_all = queries.is_empty();
	let mut results = Vec::with_capacity(queries.len().max(1));
	let mut keymap = HashMap::with_capacity(queries.len().max(1));

	// First contact the origin (if we're allowed to)
	if server_keys.notary_may_contact_origin(&remote) {
		debug_info!("Asking remote directly for verify keys");
		if let Ok(res) = server_keys.origin_request(remote.clone(), start).await
			&& let Ok(ssk) = sign_ssk(&server_keys, res.clone(), &remote, &my_name).await
		{
			let index = results.len();
			results.push(ssk);
			for key_id in res.verify_keys.keys().chain(res.old_verify_keys.keys()) {
				if fetch_all
					|| queries.get(key_id).is_some_and(|c| {
						c.minimum_valid_until_ts
							.is_none_or(|m| res.valid_until_ts >= m)
					}) {
					queries.remove(key_id);
					keymap.insert(key_id.to_owned(), index);
				}
			}
		}
	} else {
		debug!("Not asking remote for keys (already asked recently)");
	}
	debug!(keys=?keymap.keys(), "Live verify keys");
	if fetch_all && keymap.is_empty() {
		// If the server asked for all keys, AND we didn't get anything from the
		// origin, just fetch any fresh responses we have.
		server_keys
			.signing_keys_for(&remote)
			.broad_filter_map(|ssk| {
				let server_name = &remote;
				let my_name = &my_name;
				let server_keys = &server_keys;
				async move {
					if ssk.valid_until_ts > in_one_week() || ssk.valid_until_ts < start {
						return None;
					}

					sign_ssk(server_keys, ssk.clone(), server_name, my_name)
						.await
						.map(|signed_ssk| (ssk, signed_ssk))
						.ok()
				}
			})
			.ready_for_each(|(ssk, signed_ssk)| {
				let idx = results.len();
				results.push(signed_ssk);
				for key_id in ssk.verify_keys.keys().chain(ssk.old_verify_keys.keys()) {
					keymap.insert(key_id.to_owned(), idx);
				}
			})
			.await;
	}

	// If we're still missing some keys, fetch them from the local cache
	for (key_id, criteria) in queries {
		debug!(%key_id, "Fetching verify key from local repository");

		if keymap.contains_key(&key_id) {
			debug!(%key_id, "Already found key");
			continue;
		}

		let minimum_valid_until_ts = criteria
			.clone()
			.minimum_valid_until_ts
			.unwrap_or_else(|| MilliSecondsSinceUnixEpoch(uint!(0)));

		let Some(ssk) = server_keys.get_signing_key(&remote, &key_id).await else {
			debug!(%remote, %key_id, "Could not find a matching signing key locally.");
			continue;
		};

		if ssk.valid_until_ts < minimum_valid_until_ts || ssk.valid_until_ts > in_one_week() {
			debug!(
				%key_id,
				valid_until_ts=?ssk.valid_until_ts,
				?minimum_valid_until_ts,
				"Stored verify key does not satisfy query criteria"
			);
			continue;
		}
		if let Some(old_verify_key) = ssk.old_verify_keys.get(&key_id) {
			if old_verify_key.expired_ts < minimum_valid_until_ts {
				debug!(
					%key_id,
					expired_ts=?old_verify_key.expired_ts,
					?minimum_valid_until_ts,
					"Stored old verify key does not satisfy query criteria"
				);
				continue;
			}
		}

		debug!(%key_id, ?ssk, "Found key locally");
		let rep_key_ids = ssk.verify_keys.keys().cloned().collect::<Vec<_>>();
		let Ok(signed_ssk) = sign_ssk(&server_keys, ssk.clone(), &remote, &my_name)
			.await
			.inspect_err(
				|e| error!(%key_id, %remote, "Failed to sign signing keys chunk: {e:?}"),
			)
		else {
			continue;
		};

		let idx = results.len();
		results.push(signed_ssk);
		keymap.insert(key_id, idx);
		for key_id in rep_key_ids {
			keymap.insert(key_id, idx);
		}
	}

	keymap
		.into_values()
		.collect::<HashSet<_>>()
		.into_iter()
		.map(|idx| results.index(idx).to_owned())
		.collect()
}

pub(crate) async fn get_remote_server_keys_route(
	State(services): State<crate::State>,
	body: Ruma<get_remote_server_keys::v2::Request>,
) -> Result<get_remote_server_keys::v2::Response> {
	let min_valid_ts = body.minimum_valid_until_ts;
	if services
		.server_keys
		.notary_may_contact_origin(&body.server_name)
		&& let Ok(response) = services
			.server_keys
			.origin_request(body.server_name.clone(), min_valid_ts)
			.await
	{
		return sign_ssk(
			&services.server_keys,
			response,
			&body.server_name,
			services.globals.server_name(),
		)
		.await
		.map(|r| Ok(get_remote_server_keys::v2::Response::new(vec![r])))?;
	}

	let response = services
		.server_keys
		.signing_keys_for(&body.server_name)
		.broad_filter_map(|ssk| {
			let server_name = body.server_name.clone();
			async move {
				if ssk.valid_until_ts > in_one_week() || ssk.valid_until_ts < min_valid_ts {
					return None;
				}

				sign_ssk(
					&services.server_keys,
					ssk,
					server_name.as_ref(),
					services.globals.server_name(),
				)
				.await
				.ok()
			}
		})
		.collect::<Vec<_>>()
		.await;
	Ok(get_remote_server_keys::v2::Response::new(response))
}
