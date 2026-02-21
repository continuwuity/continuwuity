use std::{
	collections::{BTreeMap, HashMap, HashSet},
	net::IpAddr,
	time::{Duration, Instant},
};

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Error, Result, debug, debug_warn, err, error,
	result::LogErr,
	state_res::lexicographical_topological_sort,
	trace,
	utils::{
		IterStream, ReadyExt, millis_since_unix_epoch,
		stream::{BroadbandExt, TryBroadbandExt, automatic_width},
	},
};
use conduwuit_service::{
	Services,
	sending::{EDU_LIMIT, PDU_LIMIT},
};
use futures::{FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt};
use itertools::Itertools;
use ruma::{
	CanonicalJsonObject, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedUserId,
	RoomId, ServerName, UserId,
	api::{
		client::error::{ErrorKind, ErrorKind::LimitExceeded},
		federation::transactions::{
			edu::{
				DeviceListUpdateContent, DirectDeviceContent, Edu, PresenceContent,
				PresenceUpdate, ReceiptContent, ReceiptData, ReceiptMap, SigningKeyUpdateContent,
				TypingContent,
			},
			send_transaction_message,
		},
	},
	events::receipt::{ReceiptEvent, ReceiptEventContent, ReceiptType},
	int,
	serde::Raw,
	to_device::DeviceIdOrAllDevices,
	uint,
};
use service::transaction_ids::{TxnKey, WrappedTransactionResponse};
use tokio::sync::watch::{Receiver, Sender};
use tracing::instrument;

use crate::Ruma;

type ResolvedMap = BTreeMap<OwnedEventId, Result>;
type Pdu = (OwnedRoomId, OwnedEventId, CanonicalJsonObject);

/// # `PUT /_matrix/federation/v1/send/{txnId}`
///
/// Push EDUs and PDUs to this server.
pub(crate) async fn send_transaction_message_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<send_transaction_message::v1::Request>,
) -> Result<send_transaction_message::v1::Response> {
	if body.origin() != body.body.origin {
		return Err!(Request(Forbidden(
			"Not allowed to send transactions on behalf of other servers"
		)));
	}

	let txn_key = (body.origin().to_owned(), body.transaction_id.clone());

	// Did we already process this transaction
	if let Some(response) = services.transaction_ids.get_cached_txn(&txn_key) {
		return Ok(response);
	}
	// Or are currently processing it
	if let Some(receiver) = services.transaction_ids.get_active_federation_txn(&txn_key) {
		// Wait up to 50 seconds for a result
		return wait_for_result(receiver).await;
	}

	if body.pdus.len() > PDU_LIMIT {
		return Err!(Request(Forbidden(
			"Not allowed to send more than {PDU_LIMIT} PDUs in one transaction"
		)));
	}

	if body.edus.len() > EDU_LIMIT {
		return Err!(Request(Forbidden(
			"Not allowed to send more than {EDU_LIMIT} EDUs in one transaction"
		)));
	}

	let sender = services
		.transaction_ids
		.start_federation_txn(txn_key.clone())?;
	services.server.runtime().spawn(process_inbound_transaction(
		services,
		body,
		client,
		txn_key.clone(),
		sender,
	));
	let receiver = services
		.transaction_ids
		.get_active_federation_txn(&txn_key)
		.expect("just-created transaction was missing");
	wait_for_result(receiver).await
}

async fn wait_for_result(
	mut recv: Receiver<WrappedTransactionResponse>,
) -> Result<send_transaction_message::v1::Response> {
	if tokio::time::timeout(Duration::from_secs(50), recv.changed())
		.await
		.is_err()
	{
		// Took too long, return 429 to encourage the sender to try again
		return Err(Error::BadRequest(
			LimitExceeded { retry_after: None },
			"Transaction is being still being processed. Please try again later.",
		));
	}
	let value = recv.borrow_and_update();
	Ok(value.clone().expect("channel returned with no value?"))
}

#[instrument(
	skip_all,
	fields(
		id = ?body.transaction_id.as_str(),
		origin = ?body.origin()
	)
)]
async fn process_inbound_transaction(
	services: crate::State,
	body: Ruma<send_transaction_message::v1::Request>,
	client: IpAddr,
	txn_key: TxnKey,
	sender: Sender<Option<send_transaction_message::v1::Response>>,
) {
	let txn_start_time = Instant::now();
	let pdus = body
		.pdus
		.iter()
		.stream()
		.broad_then(|pdu| services.rooms.event_handler.parse_incoming_pdu(pdu))
		.inspect_err(|e| debug_warn!("Could not parse PDU: {e}"))
		.ready_filter_map(Result::ok);

	let edus = body
		.edus
		.iter()
		.map(|edu| edu.json().get())
		.map(serde_json::from_str)
		.filter_map(Result::ok)
		.stream();

	debug!(pdus = body.pdus.len(), edus = body.edus.len(), "Processing transaction",);
	let Ok(results) = handle(&services, &client, body.origin(), pdus, edus).await else {
		// TODO: handle this properly. The channel doesn't like being closed with no
		// value, returning an empty response may lie to the remote and make them
		// think we processed it properly (and lose events), but we also can't return
		// an actual error.
		drop(sender);
		services.transaction_ids.finish_federation_txn(&txn_key);
		panic!("failed to handle incoming transaction");
	};

	for (id, result) in &results {
		if let Err(e) = result {
			if matches!(e, Error::BadRequest(ErrorKind::NotFound, _)) {
				debug_warn!("Incoming PDU failed {id}: {e:?}");
			}
		}
	}

	debug!(
		pdus = body.pdus.len(),
		edus = body.edus.len(),
		elapsed = ?txn_start_time.elapsed(),
		"Finished processing transaction"
	);

	let response = send_transaction_message::v1::Response {
		pdus: results
			.into_iter()
			.map(|(e, r)| (e, r.map_err(error::sanitized_message)))
			.collect(),
	};
	services
		.transaction_ids
		.set_cached_txn(txn_key.clone(), response.clone());
	sender
		.send(Some(response))
		.expect("couldn't send response to channel");
	services.transaction_ids.finish_federation_txn(&txn_key);
	drop(sender);
}

async fn handle(
	services: &Services,
	client: &IpAddr,
	origin: &ServerName,
	pdus: impl Stream<Item = Pdu> + Send,
	edus: impl Stream<Item = Edu> + Send,
) -> Result<ResolvedMap> {
	// group pdus by room
	let pdus = pdus
		.collect()
		.map(|mut pdus: Vec<_>| {
			pdus.sort_by(|(room_a, ..), (room_b, ..)| room_a.cmp(room_b));
			pdus.into_iter()
				.into_grouping_map_by(|(room_id, ..)| room_id.clone())
				.collect()
		})
		.await;

	// we can evaluate rooms concurrently
	let results: ResolvedMap = pdus
		.into_iter()
		.try_stream()
		.broad_and_then(|(room_id, pdus): (_, Vec<_>)| {
			handle_room(services, client, origin, room_id, pdus.into_iter())
				.map_ok(Vec::into_iter)
				.map_ok(IterStream::try_stream)
		})
		.try_flatten()
		.try_collect()
		.boxed()
		.await?;

	// evaluate edus after pdus, at least for now.
	edus.for_each_concurrent(automatic_width(), |edu| handle_edu(services, client, origin, edu))
		.boxed()
		.await;

	Ok(results)
}

/// Attempts to build a localised directed acyclic graph out of the given PDUs,
/// returning them in a topologically sorted order.
///
/// This is used to attempt to process PDUs in an order that respects their
/// dependencies, however it is ultimately the sender's responsibility to send
/// them in a processable order, so this is just a best effort attempt. It does
/// not account for power levels or other tie breaks.
async fn build_local_dag(
	pdu_map: &HashMap<OwnedEventId, CanonicalJsonObject>,
) -> Result<Vec<OwnedEventId>> {
	debug_assert!(pdu_map.len() >= 2, "needless call to build_local_dag with less than 2 PDUs");
	let mut dag: HashMap<OwnedEventId, HashSet<OwnedEventId>> = HashMap::new();

	for (event_id, value) in pdu_map {
		let prev_events = value
			.get("prev_events")
			.expect("pdu must have prev_events")
			.as_array()
			.expect("prev_events must be an array")
			.iter()
			.map(|v| {
				OwnedEventId::parse(v.as_str().expect("prev_events values must be strings"))
					.expect("prev_events must be valid event IDs")
			})
			.collect::<HashSet<OwnedEventId>>();

		dag.insert(event_id.clone(), prev_events);
	}
	lexicographical_topological_sort(&dag, &|_| async {
		// Note: we don't bother fetching power levels because that would massively slow
		// this function down. This is a best-effort attempt to order events correctly
		// for processing, however ultimately that should be the sender's job.
		Ok((int!(0), MilliSecondsSinceUnixEpoch(uint!(0))))
	})
	.await
	.map_err(|e| err!("failed to resolve local graph: {e}"))
}

async fn handle_room(
	services: &Services,
	_client: &IpAddr,
	origin: &ServerName,
	room_id: OwnedRoomId,
	pdus: impl Iterator<Item = Pdu> + Send,
) -> Result<Vec<(OwnedEventId, Result)>> {
	let _room_lock = services
		.rooms
		.event_handler
		.mutex_federation
		.lock(&room_id)
		.await;

	let room_id = &room_id;
	let pdu_map: HashMap<OwnedEventId, CanonicalJsonObject> = pdus
		.into_iter()
		.map(|(_, event_id, value)| (event_id, value))
		.collect();
	let sorted_event_ids = if pdu_map.len() >= 2 {
		build_local_dag(&pdu_map).await?
	} else {
		pdu_map.keys().cloned().collect()
	};
	let mut results = Vec::with_capacity(sorted_event_ids.len());
	for event_id in sorted_event_ids {
		let value = pdu_map
			.get(&event_id)
			.expect("sorted event IDs must be from the original map")
			.clone();
		services.server.check_running()?;
		let result = services
			.rooms
			.event_handler
			.handle_incoming_pdu(origin, room_id, &event_id, value, true)
			.await
			.map(|_| ());
		results.push((event_id, result));
	}
	Ok(results)
}

async fn handle_edu(services: &Services, client: &IpAddr, origin: &ServerName, edu: Edu) {
	match edu {
		| Edu::Presence(presence) if services.server.config.allow_incoming_presence =>
			handle_edu_presence(services, client, origin, presence).await,

		| Edu::Receipt(receipt) if services.server.config.allow_incoming_read_receipts =>
			handle_edu_receipt(services, client, origin, receipt).await,

		| Edu::Typing(typing) if services.server.config.allow_incoming_typing =>
			handle_edu_typing(services, client, origin, typing).await,

		| Edu::DeviceListUpdate(content) =>
			handle_edu_device_list_update(services, client, origin, content).await,

		| Edu::DirectToDevice(content) =>
			handle_edu_direct_to_device(services, client, origin, content).await,

		| Edu::SigningKeyUpdate(content) =>
			handle_edu_signing_key_update(services, client, origin, content).await,

		| Edu::_Custom(ref _custom) => debug_warn!(?edu, "received custom/unknown EDU"),

		| _ => trace!(?edu, "skipped"),
	}
}

async fn handle_edu_presence(
	services: &Services,
	_client: &IpAddr,
	origin: &ServerName,
	presence: PresenceContent,
) {
	presence
		.push
		.into_iter()
		.stream()
		.for_each_concurrent(automatic_width(), |update| {
			handle_edu_presence_update(services, origin, update)
		})
		.await;
}

async fn handle_edu_presence_update(
	services: &Services,
	origin: &ServerName,
	update: PresenceUpdate,
) {
	if update.user_id.server_name() != origin {
		debug_warn!(
			%update.user_id, %origin,
			"received presence EDU for user not belonging to origin"
		);
		return;
	}

	services
		.presence
		.set_presence(
			&update.user_id,
			&update.presence,
			Some(update.currently_active),
			Some(update.last_active_ago),
			update.status_msg.clone(),
		)
		.await
		.log_err()
		.ok();
}

async fn handle_edu_receipt(
	services: &Services,
	_client: &IpAddr,
	origin: &ServerName,
	receipt: ReceiptContent,
) {
	receipt
		.receipts
		.into_iter()
		.stream()
		.for_each_concurrent(automatic_width(), |(room_id, room_updates)| {
			handle_edu_receipt_room(services, origin, room_id, room_updates)
		})
		.await;
}

async fn handle_edu_receipt_room(
	services: &Services,
	origin: &ServerName,
	room_id: OwnedRoomId,
	room_updates: ReceiptMap,
) {
	if services
		.rooms
		.event_handler
		.acl_check(origin, &room_id)
		.await
		.is_err()
	{
		debug_warn!(
			%origin, %room_id,
			"received read receipt EDU from ACL'd server"
		);
		return;
	}

	let room_id = &room_id;
	room_updates
		.read
		.into_iter()
		.stream()
		.for_each_concurrent(automatic_width(), |(user_id, user_updates)| async move {
			handle_edu_receipt_room_user(services, origin, room_id, &user_id, user_updates).await;
		})
		.await;
}

async fn handle_edu_receipt_room_user(
	services: &Services,
	origin: &ServerName,
	room_id: &RoomId,
	user_id: &UserId,
	user_updates: ReceiptData,
) {
	if user_id.server_name() != origin {
		debug_warn!(
			%user_id, %origin,
			"received read receipt EDU for user not belonging to origin"
		);
		return;
	}

	if !services
		.rooms
		.state_cache
		.server_in_room(origin, room_id)
		.await
	{
		debug_warn!(
			%user_id, %room_id, %origin,
			"received read receipt EDU from server who does not have a member in the room",
		);
		return;
	}

	let data = &user_updates.data;
	user_updates
		.event_ids
		.into_iter()
		.stream()
		.for_each_concurrent(automatic_width(), |event_id| async move {
			let user_data = [(user_id.to_owned(), data.clone())];
			let receipts = [(ReceiptType::Read, BTreeMap::from(user_data))];
			let content = [(event_id.clone(), BTreeMap::from(receipts))];
			services
				.rooms
				.read_receipt
				.readreceipt_update(user_id, room_id, &ReceiptEvent {
					content: ReceiptEventContent(content.into()),
					room_id: room_id.to_owned(),
				})
				.await;
		})
		.await;
}

async fn handle_edu_typing(
	services: &Services,
	_client: &IpAddr,
	origin: &ServerName,
	typing: TypingContent,
) {
	if typing.user_id.server_name() != origin {
		debug_warn!(
			%typing.user_id, %origin,
			"received typing EDU for user not belonging to origin"
		);
		return;
	}

	if services
		.rooms
		.event_handler
		.acl_check(typing.user_id.server_name(), &typing.room_id)
		.await
		.is_err()
	{
		debug_warn!(
			%typing.user_id, %typing.room_id, %origin,
			"received typing EDU for ACL'd user's server"
		);
		return;
	}

	if !services
		.rooms
		.state_cache
		.is_joined(&typing.user_id, &typing.room_id)
		.await
	{
		debug_warn!(
			%typing.user_id, %typing.room_id, %origin,
			"received typing EDU for user not in room"
		);
		return;
	}

	if typing.typing {
		let secs = services.server.config.typing_federation_timeout_s;
		let timeout = millis_since_unix_epoch().saturating_add(secs.saturating_mul(1000));

		services
			.rooms
			.typing
			.typing_add(&typing.user_id, &typing.room_id, timeout)
			.await
			.log_err()
			.ok();
	} else {
		services
			.rooms
			.typing
			.typing_remove(&typing.user_id, &typing.room_id)
			.await
			.log_err()
			.ok();
	}
}

async fn handle_edu_device_list_update(
	services: &Services,
	_client: &IpAddr,
	origin: &ServerName,
	content: DeviceListUpdateContent,
) {
	let DeviceListUpdateContent { user_id, .. } = content;

	if user_id.server_name() != origin {
		debug_warn!(
			%user_id, %origin,
			"received device list update EDU for user not belonging to origin"
		);
		return;
	}

	services.users.mark_device_key_update(&user_id).await;
}

async fn handle_edu_direct_to_device(
	services: &Services,
	_client: &IpAddr,
	origin: &ServerName,
	content: DirectDeviceContent,
) {
	let DirectDeviceContent {
		ref sender,
		ref ev_type,
		ref message_id,
		messages,
	} = content;

	if sender.server_name() != origin {
		debug_warn!(
			%sender, %origin,
			"received direct to device EDU for user not belonging to origin"
		);
		return;
	}

	// Check if this is a new transaction id
	if services
		.transaction_ids
		.get_client_txn(sender, None, message_id)
		.await
		.is_ok()
	{
		return;
	}

	// process messages concurrently for different users
	let ev_type = ev_type.to_string();
	messages
		.into_iter()
		.stream()
		.for_each_concurrent(automatic_width(), |(target_user_id, map)| {
			handle_edu_direct_to_device_user(services, target_user_id, sender, &ev_type, map)
		})
		.await;

	// Save transaction id with empty data
	services
		.transaction_ids
		.add_client_txnid(sender, None, message_id, &[]);
}

async fn handle_edu_direct_to_device_user<Event: Send + Sync>(
	services: &Services,
	target_user_id: OwnedUserId,
	sender: &UserId,
	ev_type: &str,
	map: BTreeMap<DeviceIdOrAllDevices, Raw<Event>>,
) {
	for (target_device_id_maybe, event) in map {
		let Ok(event) = event
			.deserialize_as()
			.map_err(|e| err!(Request(InvalidParam(error!("To-Device event is invalid: {e}")))))
		else {
			continue;
		};

		handle_edu_direct_to_device_event(
			services,
			&target_user_id,
			sender,
			target_device_id_maybe,
			ev_type,
			event,
		)
		.await;
	}
}

async fn handle_edu_direct_to_device_event(
	services: &Services,
	target_user_id: &UserId,
	sender: &UserId,
	target_device_id_maybe: DeviceIdOrAllDevices,
	ev_type: &str,
	event: serde_json::Value,
) {
	match target_device_id_maybe {
		| DeviceIdOrAllDevices::DeviceId(ref target_device_id) => {
			services
				.users
				.add_to_device_event(sender, target_user_id, target_device_id, ev_type, event)
				.await;
		},

		| DeviceIdOrAllDevices::AllDevices => {
			services
				.users
				.all_device_ids(target_user_id)
				.for_each(|target_device_id| {
					services.users.add_to_device_event(
						sender,
						target_user_id,
						target_device_id,
						ev_type,
						event.clone(),
					)
				})
				.await;
		},
	}
}

async fn handle_edu_signing_key_update(
	services: &Services,
	_client: &IpAddr,
	origin: &ServerName,
	content: SigningKeyUpdateContent,
) {
	let SigningKeyUpdateContent { user_id, master_key, self_signing_key } = content;

	if user_id.server_name() != origin {
		debug_warn!(
			%user_id, %origin,
			"received signing key update EDU from server that does not belong to user's server"
		);
		return;
	}

	services
		.users
		.add_cross_signing_keys(&user_id, &master_key, &self_signing_key, &None, true)
		.await
		.log_err()
		.ok();
}
