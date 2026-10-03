use std::{collections::BTreeMap, mem::take, time::Duration};

use axum::{Json, extract::State, response::IntoResponse};
use conduwuit::{
	Err, Result,
	utils::{stream::BroadbandExt, timepoint_from_now, to_canonical_object},
};
use futures::StreamExt;
use ruma::{
	MilliSecondsSinceUnixEpoch, ServerName,
	api::{
		OutgoingResponseExt,
		federation::discovery::{
			OldVerifyKey, ServerSigningKeys, get_remote_server_keys,
			get_remote_server_keys_batch, get_server_keys,
		},
	},
	assign,
	serde::Raw,
};
use serde_json::value::to_raw_value;
use service::{Services, server_keys, server_keys::in_one_week};

use crate::router::Ruma;

/// # `GET /_matrix/key/v2/server`
///
/// Gets the public signing keys of this server.
pub(crate) async fn get_server_keys_route(
	State(services): State<crate::State>,
) -> Result<impl IntoResponse> {
	let server_name = services.globals.server_name();
	let verify_keys = BTreeMap::from([services.server_keys.active_verify_key()]);

	let old_verify_keys = if let Some(k) = &services.config.old_verify_keys {
		k.to_owned()
	} else {
		// We need to figure out any old signing keys we've acquired from
		// notaries.
		let mut keys = services.server_keys.signing_keys_for(server_name);
		let mut old_keys = BTreeMap::new();
		while let Some(resp) = keys.next().await {
			// We need to verify that this response is still trusted based on
			// the current configuration. It's possible we got this response
			// from a notary who we no longer trust.
			if resp.valid_until_ts > in_one_week() {
				continue;
			}
			let trusted = services.config.trusted_servers.iter().any(|notary| {
				server_keys::Service::verify_server_keys_response(
					&resp,
					Some((notary.server_name(), notary.verify_keys())),
				)
				.is_ok()
			});
			if !trusted {
				continue;
			}
			for (old_key_id, old_key) in resp.old_verify_keys {
				if old_key.expired_ts > resp.valid_until_ts {
					// This key expired in the future which is probably illegal
					continue;
				}
				old_keys
					.entry(old_key_id)
					.and_modify(|current_old_key: &mut OldVerifyKey| {
						// Use the lowest expiry timestamp
						if current_old_key.expired_ts > old_key.expired_ts {
							*current_old_key = old_key.clone();
						}
					})
					.or_insert(old_key);
			}
		}
		old_keys
	};

	let server_key = assign!(ServerSigningKeys::new(server_name.to_owned(), valid_until_ts()), {
		verify_keys,
		old_verify_keys,
	});

	let server_key = Raw::new(&server_key)?;
	let mut response = get_server_keys::v2::Response::new(server_key)
		.try_into_http_response::<Vec<u8>>()
		.map(|mut response| take(response.body_mut()))
		.and_then(|body| serde_json::from_slice(&body).map_err(Into::into))?;

	services.server_keys.sign_json(&mut response)?;

	Ok(Json(response))
}

fn valid_until_ts() -> MilliSecondsSinceUnixEpoch {
	let dur = Duration::from_hours(12);
	let timepoint = timepoint_from_now(dur).expect("SystemTime should not overflow");
	MilliSecondsSinceUnixEpoch::from_system_time(timepoint).expect("UInt should not overflow")
}

const MAX_KEYS_PER_QUERY: usize = 16 * 1024;

pub(crate) async fn get_remote_server_keys_batch_route(
	State(services): State<crate::State>,
	body: Ruma<get_remote_server_keys_batch::v2::Request>,
) -> Result<get_remote_server_keys_batch::v2::Response> {
	let total_queried_keys = body
		.server_keys
		.values()
		.fold(0_usize, |acc, q| acc.saturating_add(q.len()));

	if total_queried_keys > MAX_KEYS_PER_QUERY {
		return Err!(Request(Forbidden(
			"Too many keys requested ({total_queried_keys} > {MAX_KEYS_PER_QUERY})"
		)));
	} else if total_queried_keys == 0 {
		return Ok(get_remote_server_keys_batch::v2::Response::new(Vec::new()));
	}

	let mut response = Vec::with_capacity(total_queried_keys);

	for (server_name, queries) in &body.server_keys {
		if queries.is_empty() {
			// Fetch any in-date signing key responses
			response.extend(
				services
					.server_keys
					.signing_keys_for(server_name)
					.broad_filter_map(|ssk| async move {
						sign_ssk(&services.clone(), ssk, server_name).await.ok()
					})
					.collect::<Vec<_>>()
					.await,
			);
			continue;
		}

		// Fetch any signing key responses associated with the given key_id, if
		// they're in-date
		for (key_id, criteria) in queries {
			let minimum_valid_until_ts = criteria
				.minimum_valid_until_ts
				.unwrap_or_else(MilliSecondsSinceUnixEpoch::now);
			let Some(ssk) = services
				.server_keys
				.get_signing_key(server_name, key_id)
				.await
			else {
				continue;
			};
			if ssk.valid_until_ts < minimum_valid_until_ts || ssk.valid_until_ts > in_one_week() {
				continue;
			}
			response.push(sign_ssk(&services, ssk, server_name).await?);
		}
	}

	Ok(get_remote_server_keys_batch::v2::Response::new(response))
}

async fn sign_ssk(
	services: &Services,
	ssk: ServerSigningKeys,
	server_name: &ServerName,
) -> Result<Raw<ServerSigningKeys>> {
	let mut canonical = to_canonical_object(&ssk)?;
	services.server_keys.sign_json(&mut canonical)?;
	server_keys::strip_extraneous_signatures(&mut canonical, server_name, &[services
		.globals
		.server_name()]);
	to_raw_value(&canonical)
		.map(Raw::<ServerSigningKeys>::from_json)
		.map_err(Into::into)
}

pub(crate) async fn get_remote_server_keys_route(
	State(services): State<crate::State>,
	body: Ruma<get_remote_server_keys::v2::Request>,
) -> Result<get_remote_server_keys::v2::Response> {
	let min_valid_ts = body.minimum_valid_until_ts;
	if let Ok(response) = services
		.server_keys
		.origin_request(body.server_name.clone(), min_valid_ts)
		.await
	{
		return sign_ssk(&services, response, &body.server_name)
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

				sign_ssk(&services, ssk, server_name.as_ref()).await.ok()
			}
		})
		.collect::<Vec<_>>()
		.await;
	Ok(get_remote_server_keys::v2::Response::new(response))
}
