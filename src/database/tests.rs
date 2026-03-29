#![allow(clippy::needless_borrows_for_generic_args)]

use std::{borrow::Cow, fmt::Debug};

use conduwuit::{
	arrayvec::ArrayVec,
	ruma::{EventId, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId, room_id, serde::Raw, user_id},
};
use serde::Serialize;

use crate::{
	Ignore, Interfix, de, ser,
	ser::{Json, serialize_to_vec},
};

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "serializing string at the top-level"))]
fn ser_str() {
	let user_id = user_id!("@user:example.com");
	let s = serialize_to_vec(&user_id).expect("failed to serialize user_id");
	assert_eq!(&s, user_id.as_bytes());
}

#[test]
fn ser_tuple() {
	let user_id = user_id!("@user:example.com");
	let room_id = room_id!("!room:example.com");

	let mut a = user_id.as_bytes().to_vec();
	a.push(0xFF);
	a.extend_from_slice(room_id.as_bytes());

	let b = (user_id, room_id);
	let b = serialize_to_vec(&b).expect("failed to serialize tuple");

	assert_eq!(a, b);
}

#[test]
fn ser_tuple_option() {
	let room_id = room_id!("!room:example.com");
	let user_id = user_id!("@user:example.com");

	let mut a = Vec::<u8>::new();
	a.push(0xFF);
	a.extend_from_slice(user_id.as_bytes());

	let mut aa = Vec::<u8>::new();
	aa.extend_from_slice(room_id.as_bytes());
	aa.push(0xFF);
	aa.extend_from_slice(user_id.as_bytes());

	let b: (Option<&RoomId>, &UserId) = (None, user_id);
	let b = serialize_to_vec(&b).expect("failed to serialize tuple");
	assert_eq!(a, b);

	let bb: (Option<&RoomId>, &UserId) = (Some(room_id), user_id);
	let bb = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bb);
}

#[test]
#[should_panic(expected = "I/O error: failed to write whole buffer")]
fn ser_overflow() {
	const BUFSIZE: usize = 10;

	let user_id = user_id!("@user:example.com");
	let room_id = room_id!("!room:example.com");

	assert!(BUFSIZE < user_id.as_str().len() + room_id.as_str().len());
	let mut buf = ArrayVec::<u8, BUFSIZE>::new();

	let val = (user_id, room_id);
	_ = ser::serialize(&mut buf, val).unwrap();
}

#[test]
fn ser_json() {
	use conduwuit::ruma::api::client::filter::FilterDefinition;

	let mut filter = FilterDefinition::default();
	filter.event_fields = Some(vec!["content.body".to_owned()]);

	let serialized = serialize_to_vec(Json(&filter)).expect("failed to serialize value");

	let s = String::from_utf8_lossy(&serialized);
	assert_eq!(&s, r#"{"event_fields":["content.body"]}"#);
}

#[test]
fn ser_json_value() {
	use conduwuit::ruma::api::client::filter::FilterDefinition;

	let mut filter = FilterDefinition::default();
	filter.event_fields = Some(vec!["content.body".to_owned()]);

	let value = serde_json::to_value(filter).expect("failed to serialize to serde_json::value");
	let serialized = serialize_to_vec(Json(value)).expect("failed to serialize value");

	let s = String::from_utf8_lossy(&serialized);
	assert_eq!(&s, r#"{"event_fields":["content.body"]}"#);
}

#[test]
fn ser_json_macro() {
	use serde_json::json;

	#[derive(Serialize)]
	struct Foo {
		foo: String,
	}

	let content = Foo { foo: "bar".to_owned() };
	let content = serde_json::to_value(content).expect("failed to serialize content");
	let sender: &UserId = "@foo:example.com".try_into().unwrap();
	let serialized = serialize_to_vec(Json(json!({
		"content": content,
		"sender": sender,
	})))
	.expect("failed to serialize value");

	let s = String::from_utf8_lossy(&serialized);
	assert_eq!(&s, r#"{"content":{"foo":"bar"},"sender":"@foo:example.com"}"#);
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "serializing string at the top-level"))]
fn ser_json_raw() {
	use conduwuit::ruma::api::client::filter::FilterDefinition;

	let mut filter = FilterDefinition::default();
	filter.event_fields = Some(vec!["content.body".to_owned()]);

	let value =
		serde_json::value::to_raw_value(&filter).expect("failed to serialize to raw value");
	let a = serialize_to_vec(value.get()).expect("failed to serialize raw value");
	let s = String::from_utf8_lossy(&a);
	assert_eq!(&s, r#"{"event_fields":["content.body"]}"#);
}

#[test]
#[cfg_attr(debug_assertions, should_panic(expected = "you can skip serialization instead"))]
fn ser_json_raw_json() {
	use conduwuit::ruma::api::client::filter::FilterDefinition;

	let mut filter = FilterDefinition::default();
	filter.event_fields = Some(vec!["content.body".to_owned()]);

	let value =
		serde_json::value::to_raw_value(&filter).expect("failed to serialize to raw value");
	let a = serialize_to_vec(Json(value)).expect("failed to serialize json value");
	let s = String::from_utf8_lossy(&a);
	assert_eq!(&s, r#"{"event_fields":["content.body"]}"#);
}

#[test]
fn de_tuple() {
	let user_id = user_id!("@user:example.com");
	let room_id = room_id!("!room:example.com");

	let raw: &[u8] = b"@user:example.com\xFF!room:example.com";
	let (a, b): (OwnedUserId, OwnedRoomId) = de::from_slice(raw).expect("failed to deserialize");

	assert_eq!(a, user_id, "deserialized user_id does not match");
	assert_eq!(b, room_id, "deserialized room_id does not match");
}

#[test]
#[should_panic(expected = "failed to deserialize")]
fn de_tuple_invalid() {
	let user_id = user_id!("@user:example.com");
	let room_id = room_id!("!room:example.com");

	let raw: &[u8] = b"@user:example.com\xFF@user:example.com";
	let (a, b): (OwnedUserId, OwnedRoomId) = de::from_slice(raw).expect("failed to deserialize");

	assert_eq!(a, user_id, "deserialized user_id does not match");
	assert_eq!(b, room_id, "deserialized room_id does not match");
}

#[test]
#[should_panic(expected = "failed to deserialize")]
fn de_tuple_incomplete() {
	let user_id = user_id!("@user:example.com");

	let raw: &[u8] = b"@user:example.com";
	let (a, _): (OwnedUserId, OwnedRoomId) = de::from_slice(raw).expect("failed to deserialize");

	assert_eq!(a, user_id, "deserialized user_id does not match");
}

#[test]
#[should_panic(expected = "failed to deserialize")]
fn de_tuple_incomplete_with_sep() {
	let user_id = user_id!("@user:example.com");

	let raw: &[u8] = b"@user:example.com\xFF";
	let (a, _): (OwnedUserId, OwnedRoomId) = de::from_slice(raw).expect("failed to deserialize");

	assert_eq!(a, user_id, "deserialized user_id does not match");
}

#[test]
#[cfg_attr(
	debug_assertions,
	should_panic(expected = "deserialization failed to consume trailing bytes")
)]
fn de_tuple_unfinished() {
	let user_id = user_id!("@user:example.com");
	let room_id = room_id!("!room:example.com");

	let raw: &[u8] = b"@user:example.com\xFF!room:example.com\xFF@user:example.com";
	let (a, b): (OwnedUserId, OwnedRoomId) = de::from_slice(raw).expect("failed to deserialize");

	assert_eq!(a, user_id, "deserialized user_id does not match");
	assert_eq!(b, room_id, "deserialized room_id does not match");
}

#[test]
fn de_tuple_ignore() {
	let user_id = user_id!("@user:example.com");
	let room_id = room_id!("!room:example.com");

	let raw: &[u8] = b"@user:example.com\xFF@user2:example.net\xFF!room:example.com";
	let (a, _, c): (OwnedUserId, Ignore, OwnedRoomId) =
		de::from_slice(raw).expect("failed to deserialize");

	assert_eq!(a, user_id, "deserialized user_id does not match");
	assert_eq!(c, room_id, "deserialized room_id does not match");
}

#[test]
fn de_json_array() {
	let a = &["foo", "bar", "baz"];
	let s = serde_json::to_vec(a).expect("failed to serialize to JSON array");

	let b: Raw<Vec<Raw<String>>> = de::from_slice(&s).expect("failed to deserialize");

	let d: Vec<String> =
		serde_json::from_str(b.json().get()).expect("failed to deserialize JSON");

	for (i, a) in a.iter().enumerate() {
		assert_eq!(*a, d[i]);
	}
}

#[test]
fn de_json_raw_array() {
	let a = &["foo", "bar", "baz"];
	let s = serde_json::to_vec(a).expect("failed to serialize to JSON array");

	let b: Raw<Vec<Raw<String>>> = de::from_slice(&s).expect("failed to deserialize");

	let c: Vec<Raw<String>> =
		serde_json::from_str(b.json().get()).expect("failed to deserialize JSON");

	for (i, a) in a.iter().enumerate() {
		let c = serde_json::to_value(c[i].json()).expect("failed to deserialize JSON to string");
		assert_eq!(*a, c);
	}
}

#[test]
fn ser_array() {
	let a: u64 = 123_456;
	let b: u64 = 987_654;

	let arr: &[u64] = &[a, b];
	let vec: Vec<u64> = vec![a, b];
	let arv: ArrayVec<u64, 2> = [a, b].into();

	let mut v = Vec::new();
	v.extend_from_slice(&a.to_be_bytes());
	v.extend_from_slice(&b.to_be_bytes());

	let s = serialize_to_vec(arr).expect("failed to serialize");
	assert_eq!(&s, &v, "serialization does not match");

	let s = serialize_to_vec(arv.as_slice()).expect("failed to serialize arrayvec");
	assert_eq!(&s, &v, "arrayvec serialization does not match");

	let s = serialize_to_vec(&vec).expect("failed to serialize vec");
	assert_eq!(&s, &v, "vec serialization does not match");
}

#[test]
#[ignore = "arrayvec deserialization is not implemented (separators)"]
fn de_array() {
	let a: u64 = 123_456;
	let b: u64 = 987_654;

	let mut v: Vec<u8> = Vec::new();
	v.extend_from_slice(&a.to_be_bytes());
	v.extend_from_slice(&b.to_be_bytes());

	let arv: ArrayVec<u64, 2> = de::from_slice::<ArrayVec<u64, 2>>(v.as_slice())
		.map(TryInto::try_into)
		.expect("failed to deserialize to arrayvec")
		.expect("failed to deserialize into");

	assert_eq!(arv[0], a, "deserialized arv [0] does not match");
	assert_eq!(arv[1], b, "deserialized arv [1] does not match");

	let arr: [u64; 2] = de::from_slice::<[u64; 2]>(v.as_slice())
		.map(TryInto::try_into)
		.expect("failed to deserialize to array")
		.expect("failed to deserialize into");

	assert_eq!(arr[0], a, "deserialized arr [0] does not match");
	assert_eq!(arr[1], b, "deserialized arr [1] does not match");

	let vec: Vec<u64> = de::from_slice(v.as_slice()).expect("failed to deserialize to vec");

	assert_eq!(vec[0], a, "deserialized vec [0] does not match");
	assert_eq!(vec[1], b, "deserialized vec [1] does not match");
}

#[test]
#[ignore = "Nested sequences are not supported"]
fn de_complex() {
	type Key = (OwnedUserId, ArrayVec<u64, 2>, OwnedRoomId);

	let user_id = user_id!("@user:example.com");
	let room_id = room_id!("!room:example.com");
	let a: u64 = 123_456;
	let b: u64 = 987_654;

	let mut v = Vec::new();
	v.extend_from_slice(user_id.as_bytes());
	v.extend_from_slice(b"\xFF");
	v.extend_from_slice(&a.to_be_bytes());
	v.extend_from_slice(&b.to_be_bytes());
	v.extend_from_slice(b"\xFF");
	v.extend_from_slice(room_id.as_bytes());

	let arr: &[u64] = &[a, b];
	let key = (user_id.to_owned(), arr, room_id.to_owned());
	let s = serialize_to_vec(&key).expect("failed to serialize");

	assert_eq!(&s, &v, "serialization does not match");

	let key = (user_id.to_owned(), [a, b].into(), room_id.to_owned());
	let arr: Key = de::from_slice(&v).expect("failed to deserialize");

	assert_eq!(arr, key, "deserialization does not match");

	let arr: Key = de::from_slice(&s).expect("failed to deserialize");

	assert_eq!(arr, key, "deserialization of serialization does not match");
}

#[test]
fn serde_tuple_option_value_some() {
	let room_id = room_id!("!room:example.com");
	let user_id = user_id!("@user:example.com");

	let mut aa = Vec::<u8>::new();
	aa.extend_from_slice(room_id.as_bytes());
	aa.push(0xFF);
	aa.extend_from_slice(user_id.as_bytes());

	let bb: (OwnedRoomId, Option<OwnedUserId>) = (room_id.to_owned(), Some(user_id.to_owned()));
	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (OwnedRoomId, Option<OwnedUserId>) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(bb.1, cc.1);
	assert_eq!(cc.0, bb.0);
}

#[test]
fn serde_tuple_option_value_none() {
	let room_id = room_id!("!room:example.com");

	let mut aa = Vec::<u8>::new();
	aa.extend_from_slice(room_id.as_bytes());
	aa.push(0xFF);

	let bb: (OwnedRoomId, Option<OwnedUserId>) = (room_id.to_owned(), None);
	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (OwnedRoomId, Option<OwnedUserId>) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(None, cc.1);
	assert_eq!(cc.0, bb.0);
}

#[test]
fn serde_tuple_option_none_value() {
	let user_id = user_id!("@user:example.com");

	let mut aa = Vec::<u8>::new();
	aa.push(0xFF);
	aa.extend_from_slice(user_id.as_bytes());

	let bb: (Option<OwnedRoomId>, OwnedUserId) = (None, user_id.to_owned());
	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (Option<OwnedRoomId>, OwnedUserId) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(None, cc.0);
	assert_eq!(cc.1, bb.1);
}

#[test]
fn serde_tuple_option_some_value() {
	let room_id = room_id!("!room:example.com");
	let user_id = user_id!("@user:example.com");

	let mut aa = Vec::<u8>::new();
	aa.extend_from_slice(room_id.as_bytes());
	aa.push(0xFF);
	aa.extend_from_slice(user_id.as_bytes());

	let bb: (Option<OwnedRoomId>, OwnedUserId) = (Some(room_id.to_owned()), user_id.to_owned());
	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (Option<OwnedRoomId>, OwnedUserId) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(bb.0, cc.0);
	assert_eq!(cc.1, bb.1);
}

#[test]
fn serde_tuple_option_some_some() {
	let room_id = room_id!("!room:example.com");
	let user_id = user_id!("@user:example.com");

	let mut aa = Vec::<u8>::new();
	aa.extend_from_slice(room_id.as_bytes());
	aa.push(0xFF);
	aa.extend_from_slice(user_id.as_bytes());

	let bb: (Option<OwnedRoomId>, Option<OwnedUserId>) = (Some(room_id.to_owned()), Some(user_id.to_owned()));
	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (Option<OwnedRoomId>, Option<OwnedUserId>) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(cc.0, bb.0);
	assert_eq!(bb.1, cc.1);
}

#[test]
fn serde_tuple_option_none_none() {
	let aa = vec![0xFF];

	let bb: (Option<OwnedRoomId>, Option<OwnedUserId>) = (None, None);
	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (Option<OwnedRoomId>, Option<OwnedUserId>) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(cc.0, bb.0);
	assert_eq!(None, cc.1);
}

#[test]
fn serde_tuple_option_some_none_some() {
	let room_id = room_id!("!room:example.com");
	let user_id = user_id!("@user:example.com");

	let mut aa = Vec::<u8>::new();
	aa.extend_from_slice(room_id.as_bytes());
	aa.push(0xFF);
	aa.push(0xFF);
	aa.extend_from_slice(user_id.as_bytes());

	let bb: (Option<&RoomId>, Option<&EventId>, Option<&UserId>) =
		(Some(room_id), None, Some(user_id));

	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (Option<Cow<'_, RoomId>>, Option<Cow<'_, EventId>>, Option<Cow<'_, UserId>>) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(bb.0, cc.0.as_deref());
	assert_eq!(None, cc.1.as_deref());
	assert_eq!(bb.1, cc.1.as_deref());
	assert_eq!(bb.2, cc.2.as_deref());
}

#[test]
fn serde_tuple_option_none_none_none() {
	let aa = vec![0xFF, 0xFF];

	let bb: (Option<OwnedRoomId>, Option<OwnedEventId>, Option<OwnedUserId>) = (None, None, None);
	let bbs = serialize_to_vec(&bb).expect("failed to serialize tuple");
	assert_eq!(aa, bbs);

	let cc: (Option<OwnedRoomId>, Option<OwnedEventId>, Option<OwnedUserId>) =
		de::from_slice(&bbs).expect("failed to deserialize tuple");

	assert_eq!(None, cc.0);
	assert_eq!(bb, cc);
}
