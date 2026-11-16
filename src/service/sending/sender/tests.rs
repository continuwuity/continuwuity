use std::iter::once;

use conduwuit::Error;
use futures::{StreamExt, stream::FuturesUnordered};
use ruma::server_name;
use serde_json::json;

use super::{CurTransactionStatus, TransactionStatus};
use crate::{
	sending::{Destination, EduBuf, SendingEvent},
	test_utils::{test_services, test_services_with},
};

#[tokio::test]
async fn panicked_transaction_is_dropped_without_blaming_the_remote() {
	let (services, path, _database) = test_services().await;
	let sending = &services.sending;
	let server = server_name!("remote.example");
	let dest = Destination::Federation(server.to_owned());
	let event = SendingEvent::Edu(EduBuf::from_slice(br#"{"edu_type":"m.typing"}"#));

	sending.db.queue_requests(once((&event, &dest)));
	let in_flight: Vec<_> = sending.db.queued_requests(&dest).collect().await;
	sending.db.mark_as_active(in_flight.iter());
	let next = sending.db.queue_requests(once((&event, &dest)));

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

	drop(futures);
	drop(services);
	std::fs::remove_dir_all(path).unwrap();
	assert_eq!(active, next);
	assert_eq!(sending_next, 1);
	assert_eq!(retry_after, None);
}

#[tokio::test]
async fn restarted_worker_resends_in_flight_requests_without_trimming() {
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
	let after_restart = sending.db.active_requests_for(&dest).count().await;
	sending
		.startup_netburst(id, false, &mut futures, &mut statuses)
		.await;
	let after_startup = sending.db.active_requests_for(&dest).count().await;

	drop(futures);
	drop(services);
	std::fs::remove_dir_all(path).unwrap();
	assert_eq!(after_restart, 3);
	assert_eq!(after_startup, 1);
}
