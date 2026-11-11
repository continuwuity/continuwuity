use std::sync::Arc;

use conduwuit::{
	Config, PduEvent, Server,
	log::{Log, LogLevelReloadHandles},
	metrics::Metrics,
};
use futures::StreamExt;
use ruma::{RoomId, RoomVersionId, events::StateEventType};
use serde_json::json;

#[tokio::test]
async fn admitted_joins_do_not_disappear_after_a_merge() {
	use conduwuit::matrix::PartialPdu;
	use ruma::{
		UserId,
		events::room::{
			join_rules::{JoinRule, RoomJoinRulesEventContent},
			member::{MembershipState, RoomMemberEventContent},
		},
	};

	let (services, path) = test_services().await;
	let alice = UserId::parse("@alice:test.example").unwrap();
	let bob = UserId::parse("@bob:bob.example").unwrap();
	let charlie = UserId::parse("@charlie:charlie.example").unwrap();
	let dave = UserId::parse("@dave:dave.example").unwrap();
	let mut remote_keys = Vec::new();
	for user in [&bob, &charlie, &dave] {
		let key = ruma::signatures::Ed25519KeyPair::from_der(
			&ruma::signatures::Ed25519KeyPair::generate(),
			"test".into(),
		)
		.unwrap();
		cache_key(&services, user.server_name(), &key);
		remote_keys.push(key);
	}
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
				content: serde_json::value::to_raw_value(&json!({ "room_version": "12" }))
					.unwrap(),
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
			PartialPdu::state(
				alice.to_string(),
				&RoomMemberEventContent::new(MembershipState::Join),
			),
			&alice,
			Some(&room),
			&lock,
		)
		.await
		.unwrap();
	let rules_id = services
		.rooms
		.timeline
		.build_and_append_pdu(
			PartialPdu::state("", &RoomJoinRulesEventContent::new(JoinRule::Public)),
			&alice,
			Some(&room),
			&lock,
		)
		.await
		.unwrap();
	drop(lock);
	let rules = services.rooms.timeline.get_pdu(&rules_id).await.unwrap();
	let (dave_join, json) = signed_remote_event(
		json!({
			"room_id": room, "sender": dave, "type": "m.room.member", "state_key": dave,
			"content": { "membership": "join" }, "auth_events": [rules_id], "prev_events": [rules_id],
			"depth": rules.depth + ruma::uint!(1), "origin_server_ts": conduwuit::utils::millis_since_unix_epoch(),
		}),
		dave.server_name(),
		&remote_keys[2],
	);
	assert!(
		services
			.rooms
			.event_handler
			.handle_incoming_pdu(dave.server_name(), &room, &dave_join.event_id, json, false)
			.await
			.unwrap()
			.is_some()
	);
	let mut joins = Vec::new();
	for (user, key) in [&bob, &charlie].into_iter().zip(&remote_keys) {
		let signed = signed_remote_event(
			json!({
				"room_id": room, "sender": user, "type": "m.room.member", "state_key": user,
				"content": { "membership": "join" }, "auth_events": [rules_id], "prev_events": [dave_join.event_id],
				"depth": dave_join.depth + ruma::uint!(1), "origin_server_ts": conduwuit::utils::millis_since_unix_epoch(),
			}),
			user.server_name(),
			key,
		);
		joins.push(signed);
	}
	for ((pdu, json), user) in joins.iter().zip([&bob, &charlie]) {
		let accepted = services
			.rooms
			.event_handler
			.handle_incoming_pdu(user.server_name(), &room, &pdu.event_id, json.clone(), false)
			.await
			.unwrap();
		assert!(accepted.is_some());
	}
	let bob_hash = services
		.rooms
		.state_accessor
		.pdu_shortstatehash(&joins[0].0.event_id)
		.await
		.unwrap();
	let charlie_hash = services
		.rooms
		.state_accessor
		.pdu_shortstatehash(&joins[1].0.event_id)
		.await
		.unwrap();
	assert_eq!(bob_hash, charlie_hash);
	let (merge, json) = signed_remote_event(
		json!({
			"room_id": room, "sender": dave, "type": "m.room.message",
			"content": { "msgtype": "m.text", "body": "merge" },
			"auth_events": [dave_join.event_id],
			"prev_events": [joins[0].0.event_id, joins[1].0.event_id],
			"depth": joins[0].0.depth + ruma::uint!(1),
			"origin_server_ts": conduwuit::utils::millis_since_unix_epoch(),
		}),
		dave.server_name(),
		&remote_keys[2],
	);
	assert!(
		services
			.rooms
			.event_handler
			.handle_incoming_pdu(dave.server_name(), &room, &merge.event_id, json, false)
			.await
			.unwrap()
			.is_some()
	);
	let mut results = Vec::new();
	for ((user, key), (join, _)) in [&bob, &charlie].into_iter().zip(&remote_keys).zip(&joins) {
		let membership = services
			.rooms
			.state_accessor
			.room_state_get_content::<RoomMemberEventContent>(
				&room,
				&StateEventType::RoomMember,
				user.as_str(),
			)
			.await
			.unwrap();
		assert_eq!(membership.membership, MembershipState::Join);
		let prev_events = services
			.rooms
			.state
			.get_forward_extremities(&room)
			.collect::<Vec<_>>()
			.await;
		let depth = services
			.rooms
			.timeline
			.get_pdu(&prev_events[0])
			.await
			.unwrap()
			.depth
			+ ruma::uint!(1);
		let (message, json) = signed_remote_event(
			json!({
				"room_id": room, "sender": user, "type": "m.room.message",
				"content": { "msgtype": "m.text", "body": format!("message from {user}") },
				"auth_events": [join.event_id], "prev_events": prev_events,
				"depth": depth, "origin_server_ts": conduwuit::utils::millis_since_unix_epoch(),
			}),
			user.server_name(),
			key,
		);
		let result = services
			.rooms
			.event_handler
			.handle_incoming_pdu(user.server_name(), &room, &message.event_id, json, false)
			.await;
		results.push(result.is_ok_and(|pdu| pdu.is_some()));
	}
	services.stop().await;
	drop(services);
	std::fs::remove_dir_all(path).unwrap();
	assert_eq!(results, [true, true], "both joined users must be able to send messages");
}

async fn test_services() -> (Arc<crate::Services>, std::path::PathBuf) {
	let _ = rustls::crypto::ring::default_provider().install_default();
	let path = std::env::temp_dir()
		.join(format!("continuwuity-shared-state-{:016x}", rand::random::<u64>()));
	std::fs::create_dir(&path).unwrap();
	let config: Config = serde_json::from_value(json!({
		"server_name": "test.example",
		"database_path": path,
		"db_cache_capacity_mb": 16.0,
		"rocksdb_write_buffer_capacity_mb": 4.0,
		"allow_local_presence": false,
		"dns_servers": ["127.0.0.1"],
		"allow_invalid_tls_certificates_yes_i_know_what_the_fuck_i_am_doing_with_this_and_i_know_this_is_insecure": true
	}))
	.unwrap();
	let runtime = tokio::runtime::Handle::current();
	let server = Arc::new(Server::new(
		config,
		Some(runtime.clone()),
		Log {
			reload: LogLevelReloadHandles::default(),
			capture: Arc::default(),
		},
		Metrics::new(Some(runtime), opentelemetry::global::meter("test"), None),
	));
	let services = crate::Services::build(server).await.unwrap();
	(services, path)
}

fn cache_key(
	services: &crate::Services,
	server: &ruma::ServerName,
	key: &ruma::signatures::Ed25519KeyPair,
) {
	let mut response = ruma::api::federation::discovery::ServerSigningKeys::new(
		server.to_owned(),
		crate::server_keys::in_one_week(),
	);
	let id: ruma::OwnedServerSigningKeyId =
		format!("ed25519:{}", key.version()).try_into().unwrap();
	response.verify_keys.insert(
		id.clone(),
		ruma::api::federation::discovery::VerifyKey::new(ruma::serde::Base64::new(
			key.public_key().to_vec(),
		)),
	);
	services.db["servernamekeyid_response"]
		.put((server.as_str(), id.as_str()), database::Json(&response));
}

fn signed_remote_event(
	value: serde_json::Value,
	server: &ruma::ServerName,
	key: &ruma::signatures::Ed25519KeyPair,
) -> (PduEvent, ruma::CanonicalJsonObject) {
	let rules = RoomVersionId::V12.rules().unwrap();
	let mut value = serde_json::from_value(value).unwrap();
	ruma::signatures::hash_and_sign_event(server.as_str(), key, &mut value, &rules.redaction)
		.unwrap();
	let id = conduwuit::matrix::event::gen_event_id(&value, &rules).unwrap();
	let pdu = PduEvent::from_id_val(&id, value.clone()).unwrap();
	(pdu, value)
}
