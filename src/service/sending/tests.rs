use std::{iter::once, time::Duration};

use futures::StreamExt;
use ruma::server_name;

use super::{Destination, EduBuf, SendingEvent, restart_delay};
use crate::test_utils::test_services;

#[tokio::test]
async fn queued_heads_lists_the_oldest_unsent_request_per_destination() {
	let (services, path, _database) = test_services().await;
	let db = &services.sending.db;
	let queued = Destination::Federation(server_name!("queued.example").to_owned());
	let active = Destination::Federation(server_name!("active.example").to_owned());
	let event = SendingEvent::Edu(EduBuf::from_slice(br#"{"edu_type":"m.typing"}"#));

	let first = db.queue_requests(once((&event, &queued)));
	db.queue_requests(once((&event, &queued)));
	db.queue_requests(once((&event, &active)));
	let in_flight: Vec<_> = db.queued_requests(&active).collect().await;
	db.mark_as_active(in_flight.iter());

	let heads: Vec<_> = db
		.queued_heads()
		.await
		.into_iter()
		.map(|(dest, (key, _))| (dest, key))
		.collect();

	drop(services);
	std::fs::remove_dir_all(path).unwrap();
	assert_eq!(heads, [(queued, first[0].clone())]);
}

#[test]
fn restart_delay_doubles_up_to_the_cap() {
	assert_eq!(restart_delay(1), Duration::from_millis(2500));
	assert_eq!(restart_delay(2), Duration::from_secs(5));
	assert_eq!(restart_delay(4), Duration::from_secs(20));
	assert_eq!(restart_delay(5), Duration::from_secs(30));
	assert_eq!(restart_delay(u32::MAX), Duration::from_secs(30));
}
