use std::{collections::BTreeMap, iter::once};

use conduwuit::{
	Error,
	matrix::{PartialPdu, pdu::sticky},
};
use futures::{StreamExt, stream::FuturesUnordered};
use ruma::{
	RoomId, UserId,
	api::federation::transactions::edu::ReceiptData,
	events::{receipt::Receipt, sticky::StickyDurationMs},
	server_name, user_id,
};
use serde_json::{json, value::to_raw_value};

use super::{CurTransactionStatus, TransactionStatus, add_receipt};
use crate::{
	sending::{Destination, EduBuf, SendingEvent},
	test_utils::{test_services, test_services_with},
};

#[tokio::test]
async fn panicked_transaction_is_kept_for_retry_without_blaming_the_remote() {
	let (services, path, _database) = test_services().await;
	let sending = &services.sending;
	let server = server_name!("remote.example");
	let dest = Destination::Federation(server.to_owned());
	let event = SendingEvent::Edu(EduBuf::from_slice(br#"{"edu_type":"m.typing"}"#));

	sending.db.queue_requests(once((&event, &dest)));
	let in_flight: Vec<_> = sending.db.queued_requests(&dest).collect().await;
	sending.db.mark_as_active(in_flight.iter());

	let mut futures = FuturesUnordered::new();
	let mut statuses = CurTransactionStatus::from([(dest.clone(), TransactionStatus::Running)]);
	let panic = Error::from_panic(Box::new("boom"));
	sending
		.handle_response(Err((dest.clone(), Box::new(panic))), &mut futures, &mut statuses)
		.await;

	let active: Vec<_> = sending
		.db
		.active_requests_for(&dest)
		.map(|(key, _)| key)
		.collect()
		.await;
	let retry_after = sending.services.federation.retry_after(server);
	let sending_next = futures.len();
	let failed = matches!(statuses.get(&dest), Some(TransactionStatus::Failed(1, _, true)));
	let immediate_retry = sending.should_attempt_send(&dest, &mut statuses).unwrap();

	drop(futures);
	drop(services);
	std::fs::remove_dir_all(path).unwrap();
	assert_eq!(
		active,
		in_flight
			.into_iter()
			.map(|(key, _)| key)
			.collect::<Vec<_>>()
	);
	assert_eq!(sending_next, 0);
	assert!(failed);
	assert_eq!(immediate_retry, (false, false));
	assert_eq!(retry_after, None);
}

#[tokio::test]
async fn restarted_worker_resends_trimmed_in_flight_requests() {
	let (services, path, _database) =
		test_services_with(json!({ "startup_netburst_keep": 1 })).await;
	let sending = &services.sending;
	let dest = Destination::Federation(server_name!("remote.example").to_owned());
	let event = SendingEvent::Edu(EduBuf::from_slice(br#"{"edu_type":"m.typing"}"#));

	for _ in 0..3 {
		sending.db.queue_requests(once((&event, &dest)));
	}
	let queued: Vec<_> = sending.db.queued_requests(&dest).collect().await;
	sending.db.mark_as_active(queued.iter());

	let id = sending.shard_id(&dest);
	let mut futures = FuturesUnordered::new();
	let mut statuses = CurTransactionStatus::new();
	sending
		.startup_netburst(id, true, &mut futures, &mut statuses)
		.await;
	let active = sending.db.active_requests_for(&dest).count().await;
	let sending_now = futures.len();

	drop(futures);
	drop(services);
	std::fs::remove_dir_all(path).unwrap();
	assert_eq!(active, 1);
	assert_eq!(sending_now, 1);
}

#[tokio::test]
async fn restarted_worker_keeps_sticky_events_when_startup_netburst_is_disabled() {
	let (services, path, _database) =
		test_services_with(json!({ "startup_netburst": false, "startup_netburst_keep": 0 }))
			.await;
	let alice = UserId::parse("@alice:test.example").unwrap();
	let bootstrap = services
		.rooms
		.state
		.mutex
		.lock("!bootstrap:test.example")
		.await;
	let create_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu {
				event_type: "m.room.create".into(),
				content: to_raw_value(&json!({ "room_version": "12" })).unwrap(),
				state_key: Some("".into()),
				..Default::default()
			},
			&alice,
			None,
			&bootstrap,
		)
		.await
		.unwrap();
	drop(bootstrap);

	let room = RoomId::parse(create_id.as_str().replacen('$', "!", 1)).unwrap();
	let lock = services.rooms.state.mutex.lock(&room).await;
	services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu {
				event_type: "m.room.member".into(),
				content: to_raw_value(&json!({ "membership": "join" })).unwrap(),
				state_key: Some(alice.as_str().into()),
				..Default::default()
			},
			&alice,
			Some(&room),
			&lock,
		)
		.await
		.unwrap();
	let sticky_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu {
				event_type: "m.room.message".into(),
				content: to_raw_value(&json!({ "msgtype": "m.text", "body": "sticky" })).unwrap(),
				sticky: Some(sticky::object(StickyDurationMs::new_clamped(600_000_u32))),
				..Default::default()
			},
			&alice,
			Some(&room),
			&lock,
		)
		.await
		.unwrap();
	drop(lock);

	let raw_pdu_id = services
		.rooms
		.timeline
		.get_pdu_id(&sticky_id)
		.await
		.unwrap();
	let sending = &services.sending;
	let dest = Destination::Federation(server_name!("remote.example").to_owned());
	let event = SendingEvent::Pdu(raw_pdu_id);

	sending.db.queue_requests(once((&event, &dest)));
	let queued: Vec<_> = sending.db.queued_requests(&dest).collect().await;
	sending.db.mark_as_active(queued.iter());

	let id = sending.shard_id(&dest);
	let mut futures = FuturesUnordered::new();
	let mut statuses = CurTransactionStatus::new();
	sending
		.startup_netburst(id, true, &mut futures, &mut statuses)
		.await;
	let active_after_restart = sending.db.active_requests_for(&dest).count().await;
	let sending_after_restart = futures.len();

	drop(futures);
	let mut futures = FuturesUnordered::new();
	let mut statuses = CurTransactionStatus::new();
	sending
		.startup_netburst(id, false, &mut futures, &mut statuses)
		.await;
	let active_after_startup = sending.db.active_requests_for(&dest).count().await;
	let sending_after_startup = futures.len();

	drop(futures);
	drop(services);
	std::fs::remove_dir_all(path).unwrap();
	assert_eq!(active_after_restart, 1);
	assert_eq!(sending_after_restart, 1);
	assert_eq!(active_after_startup, 0);
	assert_eq!(sending_after_startup, 0);
}

#[test]
fn add_receipt_separates_receipts_of_one_user() {
	let alice = user_id!("@alice:example.org");
	let bob = user_id!("@bob:example.org");
	let receipt = || ReceiptData::new(Receipt::default(), vec![]);
	let mut maps = Vec::new();

	assert_eq!(add_receipt(&mut maps, alice.to_owned(), receipt(), 2), Some(0));
	assert_eq!(add_receipt(&mut maps, alice.to_owned(), receipt(), 2), Some(1));
	assert_eq!(add_receipt(&mut maps, bob.to_owned(), receipt(), 2), Some(0));
	assert_eq!(add_receipt(&mut maps, alice.to_owned(), receipt(), 2), None);
	assert_eq!(maps.iter().map(BTreeMap::len).collect::<Vec<_>>(), [2, 1]);
}
