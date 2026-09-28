use std::{collections::BTreeMap, mem::take, time::Duration};

use axum::{Json, extract::State, response::IntoResponse};
use conduwuit::{Result, utils::timepoint_from_now};
use futures::StreamExt;
use ruma::{
	MilliSecondsSinceUnixEpoch,
	api::{
		OutgoingResponseExt,
		federation::discovery::{OldVerifyKey, ServerSigningKeys, get_server_keys},
	},
	assign,
	serde::Raw,
};
use service::{server_keys, server_keys::in_one_week};

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
