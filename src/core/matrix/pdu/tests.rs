use super::Count;

#[test]
fn backfilled_parse() {
	let count: Count = "-987654".parse().expect("parse() failed");
	let backfilled = matches!(count, Count::Backfilled(_));

	assert!(backfilled, "not backfilled variant");
}

#[test]
fn normal_parse() {
	let count: Count = "987654".parse().expect("parse() failed");
	let backfilled = matches!(count, Count::Backfilled(_));

	assert!(!backfilled, "backfilled variant");
}

fn sticky_pdu(sticky: &serde_json::Value) -> super::Pdu {
	serde_json::from_value(serde_json::json!({
		"event_id": "$test:example.com",
		"room_id": "!test:example.com",
		"sender": "@test:example.com",
		"origin_server_ts": 1_000_000,
		"type": "m.room.message",
		"content": { "msgtype": "m.text", "body": "test" },
		"msc4354_sticky": sticky,
		"prev_events": [],
		"depth": 1,
		"auth_events": [],
		"hashes": { "sha256": "test_hash" },
	}))
	.expect("PDU is valid")
}

#[test]
fn sticky_object_round_trips_verbatim() {
	// the object is signed, so serialization must not touch it
	let pdu = sticky_pdu(&serde_json::json!({ "duration_ms": 300_000, "unknown": "key" }));
	let value = serde_json::to_value(&pdu).expect("PDU serializes");

	assert_eq!(
		value["msc4354_sticky"],
		serde_json::json!({ "duration_ms": 300_000, "unknown": "key" })
	);
}

#[test]
fn out_of_range_sticky_object_is_clamped_not_rejected() {
	let pdu = sticky_pdu(&serde_json::json!({ "duration_ms": 9_999_999_999_u64 }));
	let sticky = pdu.sticky.as_deref().expect("sticky object is kept");

	assert_eq!(super::sticky::duration_ms(sticky), Some(super::sticky::MAX_DURATION_MS));
}

#[test]
fn nonsense_sticky_object_does_not_reject_the_pdu() {
	let pdu = sticky_pdu(&serde_json::json!("nonsense"));
	let sticky = pdu.sticky.as_deref().expect("sticky object is kept");

	assert_eq!(super::sticky::duration_ms(sticky), None);
}

#[test]
fn redaction_removes_stickiness() {
	let mut pdu = sticky_pdu(&serde_json::json!({ "duration_ms": 300_000 }));
	pdu.redact(&ruma::RoomVersionId::V11, serde_json::json!({}))
		.expect("redaction succeeds");

	assert!(pdu.sticky.is_none());
}

#[test]
fn sticky_ttl_is_added_to_unsigned() {
	let mut pdu = sticky_pdu(&serde_json::json!({ "duration_ms": 300_000 }));
	pdu.origin_server_ts = ruma::MilliSecondsSinceUnixEpoch::now().get();
	pdu.add_sticky_duration_ttl().expect("ttl is added");

	let unsigned: serde_json::Value =
		serde_json::from_str(pdu.unsigned.as_deref().expect("unsigned is set").get())
			.expect("unsigned is valid");
	let ttl = unsigned["msc4354_sticky_duration_ttl_ms"]
		.as_u64()
		.expect("ttl is a number");

	assert!(ttl > 0 && ttl <= 300_000, "ttl {ttl} out of range");
}

#[test]
fn no_ttl_for_events_that_are_not_sticky() {
	let mut pdu = sticky_pdu(&serde_json::json!({ "duration_ms": "nonsense" }));
	pdu.add_sticky_duration_ttl().expect("ttl is skipped");

	assert!(pdu.unsigned.is_none());
}

#[test]
fn set_unsigned_adds_the_sticky_ttl() {
	// sticky events in the timeline get the hint through here, not the sticky
	// section
	let mut pdu = sticky_pdu(&serde_json::json!({ "duration_ms": 300_000 }));
	pdu.origin_server_ts = ruma::MilliSecondsSinceUnixEpoch::now().get();
	pdu.set_unsigned(None);

	let unsigned: serde_json::Value =
		serde_json::from_str(pdu.unsigned.as_deref().expect("unsigned is set").get())
			.expect("unsigned is valid");

	assert!(unsigned["msc4354_sticky_duration_ttl_ms"].is_number());
}
