use std::{
	collections::{BTreeMap, HashMap, HashSet},
	time::Duration,
};

use axum::extract::State;
use conduwuit::{
	Err, Result, debug, debug_warn, err,
	result::FlatOk,
	utils::{IterStream, TryFutureExtExt, stream::WidebandExt},
};
use conduwuit_service::{Services, users::parse_master_key};
use futures::{StreamExt, stream::FuturesUnordered};
use ruma::{
	OneTimeKeyAlgorithm, OwnedDeviceId, OwnedUserId, UserId,
	api::{
		client::keys::{
			claim_keys, get_key_changes, get_keys, upload_keys,
			upload_signatures::{self},
			upload_signing_keys,
		},
		federation,
	},
	assign,
	encryption::CrossSigningKey,
	serde::Raw,
};
use serde_json::json;
use service::{oauth::OAuthTicket, uiaa::Identity};

use crate::Ruma;

/// # `POST /_matrix/client/r0/keys/upload`
///
/// Publish end-to-end encryption keys for the sender device.
///
/// - Adds one time keys
/// - If there are no device keys yet: Adds device keys (TODO: merge with
///   existing keys?)
pub(crate) async fn upload_keys_route(
	State(services): State<crate::State>,
	body: Ruma<upload_keys::v3::Request>,
) -> Result<upload_keys::v3::Response> {
	let (sender_user, sender_device) = body.sender();

	for (key_id, one_time_key) in &body.one_time_keys {
		if one_time_key
			.deserialize()
			.inspect_err(|e| {
				debug_warn!(
					%key_id,
					?one_time_key,
					"Invalid one time key JSON submitted by client, skipping: {e}"
				);
			})
			.is_err()
		{
			continue;
		}

		services
			.users
			.add_one_time_key(sender_user, sender_device, key_id, one_time_key)
			.await?;
	}

	for (key_id, fallback_key) in &body.fallback_keys {
		if fallback_key
			.deserialize()
			.inspect_err(|e| {
				debug_warn!(
					%key_id,
					?fallback_key,
					"Invalid one time key JSON submitted by client, skipping: {e}"
				);
			})
			.is_err()
		{
			continue;
		}

		services
			.users
			.add_fallback_key(sender_user, sender_device, key_id, fallback_key, false)
			.await?;
	}

	if let Some(device_keys) = &body.device_keys {
		let deser_device_keys = device_keys.deserialize().map_err(|e| {
			err!(Request(BadJson(debug_warn!(
				?device_keys,
				"Invalid device keys JSON uploaded by client: {e}"
			))))
		})?;

		if deser_device_keys.user_id != sender_user {
			return Err!(Request(Unknown(
				"User ID in keys uploaded does not match your own user ID"
			)));
		}
		if deser_device_keys.device_id != sender_device {
			return Err!(Request(Unknown(
				"Device ID in keys uploaded does not match your own device ID"
			)));
		}

		if let Ok(existing_keys) = services
			.users
			.get_device_keys(sender_user, sender_device)
			.await
			.and_then(|keys| keys.deserialize().map_err(Into::into))
		{
			// NOTE: also serves as a workaround for a nheko bug which omits cross-signing
			// NOTE: signatures when re-uploading the same DeviceKeys.
			if existing_keys.keys == deser_device_keys.keys {
				debug!(
					%sender_user,
					%sender_device,
					?device_keys,
					"Ignoring user uploaded keys as they are an exact copy already in the \
					 database"
				);
			} else {
				services
					.users
					.add_device_keys(sender_user, sender_device, device_keys)
					.await;
			}
		} else {
			services
				.users
				.add_device_keys(sender_user, sender_device, device_keys)
				.await;
		}
	}

	let one_time_key_counts = services
		.users
		.count_one_time_keys(sender_user, sender_device)
		.await;

	Ok(upload_keys::v3::Response::new(one_time_key_counts))
}

/// # `POST /_matrix/client/r0/keys/query`
///
/// Get end-to-end encryption keys for the given users.
///
/// - Always fetches users from other servers over federation
/// - Gets master keys, self-signing keys, user signing keys and device keys.
/// - The master and self-signing keys contain signatures that the user is
///   allowed to see
pub(crate) async fn get_keys_route(
	State(services): State<crate::State>,
	body: Ruma<get_keys::v3::Request>,
) -> Result<get_keys::v3::Response> {
	let sender_user = body.sender_user();

	get_keys_helper(
		&services,
		Some(sender_user),
		&body.device_keys,
		|u| u == sender_user,
		true, // Always allow local users to see device names of other local users
		body.timeout.unwrap_or(Duration::from_secs(10)),
	)
	.await
}

/// # `POST /_matrix/client/r0/keys/claim`
///
/// Claims one-time keys
pub(crate) async fn claim_keys_route(
	State(services): State<crate::State>,
	body: Ruma<claim_keys::v3::Request>,
) -> Result<claim_keys::v3::Response> {
	claim_keys_helper(
		&services,
		&body.one_time_keys,
		body.timeout.unwrap_or(Duration::from_secs(10)),
	)
	.await
}

/// # `POST /_matrix/client/r0/keys/device_signing/upload`
///
/// Uploads end-to-end key information for the sender user.
///
/// - Requires UIAA to verify password
pub(crate) async fn upload_signing_keys_route(
	State(services): State<crate::State>,
	body: Ruma<upload_signing_keys::v3::Request>,
) -> Result<upload_signing_keys::v3::Response> {
	let sender_user = body.sender_user();

	if uiaa_needed_to_upload_keys(
		services,
		sender_user,
		body.self_signing_key.as_ref(),
		body.user_signing_key.as_ref(),
		body.master_key.as_ref(),
	)
	.await
	{
		let _ = services
			.uiaa
			.authenticate_password(
				&body.auth,
				sender_user,
				body.sender_device(),
				Some(OAuthTicket::CrossSigningReset),
			)
			.await?;
	}

	services
		.users
		.add_cross_signing_keys(
			sender_user,
			&body.master_key,
			&body.self_signing_key,
			&body.user_signing_key,
			true, // notify so that other users see the new keys
		)
		.await?;

	Ok(upload_signing_keys::v3::Response::new())
}

async fn uiaa_needed_to_upload_keys(
	services: crate::State,
	user_id: &UserId,
	self_signing_key: Option<&Raw<CrossSigningKey>>,
	user_signing_key: Option<&Raw<CrossSigningKey>>,
	master_signing_key: Option<&Raw<CrossSigningKey>>,
) -> bool {
	let (self_signing_key, user_signing_key, master_signing_key) = (
		self_signing_key.map(Raw::deserialize).flat_ok(),
		user_signing_key.map(Raw::deserialize).flat_ok(),
		master_signing_key.map(Raw::deserialize).flat_ok(),
	);

	let (existing_self_signing_key, existing_user_signing_key, existing_master_signing_key) = futures::join!(
		services
			.users
			.get_self_signing_key(None, user_id, &|_| true)
			.ok(),
		services.users.get_user_signing_key(user_id).ok(),
		services.users.get_master_key(None, user_id, &|_| true).ok(),
	);

	let (existing_self_signing_key, existing_user_signing_key, existing_master_signing_key) = (
		existing_self_signing_key
			.as_ref()
			.map(Raw::deserialize)
			.flat_ok(),
		existing_user_signing_key
			.as_ref()
			.map(Raw::deserialize)
			.flat_ok(),
		existing_master_signing_key
			.as_ref()
			.map(Raw::deserialize)
			.flat_ok(),
	);

	if let Some(existing_master_signing_key) = existing_master_signing_key {
		// If a master key exists, UIAA is required if any of the keys are different.

		master_signing_key != Some(existing_master_signing_key)
			|| user_signing_key != existing_user_signing_key
			|| self_signing_key != existing_self_signing_key
	} else {
		// If no master key exists, UIAA is not required.
		false
	}
}

/// # `POST /_matrix/client/r0/keys/signatures/upload`
///
/// Uploads end-to-end key signatures from the sender user.
///
/// TODO: clean this timo-code up more and integrate failures. tried to improve
/// it a bit to stop exploding the entire request on bad sigs, but needs way
/// more work.
pub(crate) async fn upload_signatures_route(
	State(services): State<crate::State>,
	body: Ruma<upload_signatures::v3::Request>,
) -> Result<upload_signatures::v3::Response> {
	if body.signed_keys.is_empty() {
		debug!("Empty signed_keys sent in key signature upload");
		return Ok(upload_signatures::v3::Response::new());
	}

	let sender_user = body.sender_user();

	for (user_id, keys) in &body.signed_keys {
		for (key_id, key) in keys {
			let Ok(key) = serde_json::to_value(key)
				.inspect_err(|e| debug_warn!(%key_id, "Invalid \"key\" JSON: {e}"))
			else {
				continue;
			};

			let Some(signatures) = key.get("signatures") else {
				continue;
			};

			let Some(sender_user_val) = signatures.get(sender_user.to_string()) else {
				continue;
			};

			let Some(sender_user_object) = sender_user_val.as_object() else {
				continue;
			};

			for (signature, val) in sender_user_object.clone() {
				let Some(val) = val.as_str().map(ToOwned::to_owned) else {
					continue;
				};
				let signature = (signature, val);

				if let Err(_e) = services
					.users
					.sign_key(user_id, key_id, signature, sender_user)
					.await
					.inspect_err(|e| debug_warn!("{e}"))
				{
					continue;
				}
			}
		}
	}

	Ok(upload_signatures::v3::Response::new())
}

/// # `POST /_matrix/client/r0/keys/changes`
///
/// Gets a list of users who have updated their device identity keys since the
/// previous sync token.
///
/// - TODO: left users
pub(crate) async fn get_key_changes_route(
	State(services): State<crate::State>,
	body: Ruma<get_key_changes::v3::Request>,
) -> Result<get_key_changes::v3::Response> {
	let sender_user = body.sender_user();

	let mut device_list_updates = HashSet::new();

	let from = body
		.from
		.parse()
		.map_err(|_| err!(Request(InvalidParam("Invalid `from`."))))?;

	let to = body
		.to
		.parse()
		.map_err(|_| err!(Request(InvalidParam("Invalid `to`."))))?;

	device_list_updates.extend(
		services
			.users
			.keys_changed(sender_user, Some(from), Some(to))
			.collect::<Vec<_>>()
			.await,
	);

	let mut rooms_joined = services.rooms.state_cache.rooms_joined(sender_user).boxed();

	while let Some(room_id) = rooms_joined.next().await {
		device_list_updates.extend(
			services
				.users
				.room_keys_changed(&room_id, Some(from), Some(to))
				.map(|(user_id, _)| user_id)
				.collect::<Vec<_>>()
				.await,
		);
	}

	Ok(get_key_changes::v3::Response::new(
		device_list_updates.into_iter().collect(),
		// TODO
		vec![],
	))
}

pub(crate) async fn get_keys_helper<F>(
	services: &Services,
	sender_user: Option<&UserId>,
	device_keys_input: &BTreeMap<OwnedUserId, Vec<OwnedDeviceId>>,
	allowed_signatures: F,
	include_display_names: bool,
	timeout: Duration,
) -> Result<get_keys::v3::Response>
where
	F: Fn(&UserId) -> bool + Send + Sync,
{
	let mut master_keys = BTreeMap::new();
	let mut self_signing_keys = BTreeMap::new();
	let mut user_signing_keys = BTreeMap::new();
	let mut device_keys = BTreeMap::new();

	let mut get_over_federation = HashMap::new();

	for (user_id, device_ids) in device_keys_input {
		let user_id: &UserId = user_id;

		if !services.globals.user_is_local(user_id) {
			get_over_federation
				.entry(user_id.server_name())
				.or_insert_with(Vec::new)
				.push((user_id, device_ids));
			continue;
		}

		if device_ids.is_empty() {
			let mut container = BTreeMap::new();
			let mut devices = services.users.all_device_ids(user_id).boxed();

			while let Some(device_id) = devices.next().await {
				if let Ok(mut keys) = services.users.get_device_keys(user_id, &device_id).await {
					let metadata = services
						.users
						.get_device_metadata(user_id, &device_id)
						.await
						.map_err(|_| {
							err!(Database("all_device_keys contained nonexistent device."))
						})?;

					add_unsigned_device_display_name(&mut keys, metadata, include_display_names)
						.map_err(|_| err!(Database("invalid device keys in database")))?;

					container.insert(device_id.clone(), keys);
				}
			}

			device_keys.insert(user_id.to_owned(), container);
		} else {
			for device_id in device_ids {
				let mut container = BTreeMap::new();
				if let Ok(mut keys) = services.users.get_device_keys(user_id, device_id).await {
					let metadata = services
						.users
						.get_device_metadata(user_id, device_id)
						.await
						.map_err(|_| {
							err!(Request(InvalidParam(
								"Tried to get keys for nonexistent device."
							)))
						})?;

					add_unsigned_device_display_name(&mut keys, metadata, include_display_names)
						.map_err(|_| err!(Database("invalid device keys in database")))?;

					container.insert(device_id.to_owned(), keys);
				}

				device_keys.insert(user_id.to_owned(), container);
			}
		}

		if let Ok(master_key) = services
			.users
			.get_master_key(sender_user, user_id, &allowed_signatures)
			.await
		{
			master_keys.insert(user_id.to_owned(), master_key);
		}
		if let Ok(self_signing_key) = services
			.users
			.get_self_signing_key(sender_user, user_id, &allowed_signatures)
			.await
		{
			self_signing_keys.insert(user_id.to_owned(), self_signing_key);
		}
		if Some(user_id) == sender_user {
			if let Ok(user_signing_key) = services.users.get_user_signing_key(user_id).await {
				user_signing_keys.insert(user_id.to_owned(), user_signing_key);
			}
		}
	}

	let mut failures = BTreeMap::new();

	let futures = get_over_federation
		.into_iter()
		.stream()
		.wide_filter_map(|(server, vec)| async move {
			let mut device_keys_input_fed = BTreeMap::new();
			for (user_id, keys) in vec {
				device_keys_input_fed.insert(user_id.to_owned(), keys.clone());
			}

			let request = federation::keys::get_keys::v1::Request::new(device_keys_input_fed);
			let response = tokio::time::timeout(
				timeout,
				services.sending.send_federation_request(server, request),
			)
			.await
			// Need to flatten the Result<Result<V, E>, E> into Result<V, E>
			.map_err(|_| err!(Request(Unknown("Timeout when getting keys over federation."))))
			.and_then(|res| res);

			Some((server, response))
		})
		.collect::<FuturesUnordered<_>>()
		.await
		.into_iter();

	for (server, response) in futures {
		match response {
			| Ok(response) => {
				for (user, master_key) in response.master_keys {
					let (master_key_id, mut master_key) = parse_master_key(&user, &master_key)?;

					if let Ok(our_master_key) = services
						.users
						.get_key(&master_key_id, sender_user, &user, &allowed_signatures)
						.await
					{
						let (_, mut our_master_key) = parse_master_key(&user, &our_master_key)?;
						master_key.signatures.append(&mut our_master_key.signatures);
					}
					let json = serde_json::to_value(master_key).expect("to_value always works");
					let raw = serde_json::from_value(json).expect("Raw::from_value always works");
					services
						.users
						.add_cross_signing_keys(
							&user, &raw, &None, &None,
							false, /* Dont notify. A notification would trigger another key
							       * request resulting in an endless loop */
						)
						.await?;
					if let Some(raw) = raw {
						master_keys.insert(user.clone(), raw);
					}
				}

				self_signing_keys.extend(response.self_signing_keys);
				device_keys.extend(response.device_keys);
			},
			| Err(e) => {
				failures.insert(server.to_string(), json!({ "error": e.to_string() }));
			},
		}
	}

	Ok(assign!(get_keys::v3::Response::new(), {
		failures,
		device_keys,
		master_keys,
		self_signing_keys,
		user_signing_keys,
	}))
}

fn add_unsigned_device_display_name(
	keys: &mut Raw<ruma::encryption::DeviceKeys>,
	metadata: ruma::api::client::device::Device,
	include_display_names: bool,
) -> serde_json::Result<()> {
	if let Some(display_name) = metadata.display_name {
		let mut object =
			keys.deserialize_as_unchecked::<serde_json::Map<String, serde_json::Value>>()?;

		let unsigned = object.entry("unsigned").or_insert_with(|| json!({}));
		if let serde_json::Value::Object(unsigned_object) = unsigned {
			if include_display_names {
				unsigned_object.insert("device_display_name".to_owned(), display_name.into());
			} else {
				unsigned_object.insert(
					"device_display_name".to_owned(),
					Some(metadata.device_id.as_str().to_owned()).into(),
				);
			}
		}

		*keys = Raw::from_json(serde_json::value::to_raw_value(&object)?);
	}

	Ok(())
}

pub(crate) async fn claim_keys_helper(
	services: &Services,
	one_time_keys_input: &BTreeMap<OwnedUserId, BTreeMap<OwnedDeviceId, OneTimeKeyAlgorithm>>,
	timeout: Duration,
) -> Result<claim_keys::v3::Response> {
	let mut one_time_keys = BTreeMap::new();

	let mut get_over_federation = BTreeMap::new();

	for (user_id, map) in one_time_keys_input {
		if !services.globals.user_is_local(user_id) {
			get_over_federation
				.entry(user_id.server_name())
				.or_insert_with(Vec::new)
				.push((user_id, map));
		}

		let mut container = BTreeMap::new();
		for (device_id, key_algorithm) in map {
			if let Ok(one_time_keys) = services
				.users
				.take_one_time_key(user_id, device_id, key_algorithm)
				.await
			{
				let mut c = BTreeMap::new();
				c.insert(one_time_keys.0, one_time_keys.1);
				container.insert(device_id.clone(), c);
			}
		}
		one_time_keys.insert(user_id.clone(), container);
	}

	let mut failures = BTreeMap::new();

	let futures = get_over_federation
		.into_iter()
		.stream()
		.wide_filter_map(|(server, vec)| async move {
			let mut one_time_keys_input_fed = BTreeMap::new();
			for (user_id, keys) in vec {
				one_time_keys_input_fed.insert(user_id.clone(), keys.clone());
			}
			let response = tokio::time::timeout(
				timeout,
				services.sending.send_federation_request(
					server,
					federation::keys::claim_keys::v1::Request::new(one_time_keys_input_fed),
				),
			)
			.await
			.map_err(|_| err!(Request(Unknown("Timeout when claiming keys over federation."))))
			.and_then(|res| res);
			Some((server, response))
		})
		.collect::<FuturesUnordered<_>>()
		.await
		.into_iter();

	for (server, response) in futures {
		match response {
			| Ok(keys) => {
				one_time_keys.extend(keys.one_time_keys);
			},
			| Err(e) => {
				failures.insert(server.to_string(), json!({"error": e.to_string()}));
			},
		}
	}

	Ok(assign!(claim_keys::v3::Response::new(one_time_keys), { failures: failures }))
}
