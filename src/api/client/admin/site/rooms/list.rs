use axum::extract::State;
use conduwuit::{
	Event, Result,
	utils::stream::{BroadbandExt, WidebandExt},
};
use futures::StreamExt;
use ruma::{
	OwnedRoomId,
	events::{
		StateEventType,
		room::{
			create::RoomCreateEventContent,
			encryption::PossiblyRedactedRoomEncryptionEventContent,
			tombstone::PossiblyRedactedRoomTombstoneEventContent,
		},
	},
};
use ruminuwuity::admin::continuwuity::rooms;
use tokio::join;

use crate::Ruma;

/// # `GET /_continuwuity/admin/rooms`
///
/// Lists all room IDs known to this server, excluding banned ones.
///
/// This is the legacy version of the endpoint, which does not support
/// pagination or including banned rooms. It is recommended to use the
/// `/v1/rooms` endpoint instead. This endpoint may be removed in a future
/// release.
pub(crate) async fn legacy_list_rooms(
	State(services): State<crate::State>,
	_body: Ruma<rooms::list::unstable::Request>,
) -> Result<rooms::list::unstable::Response> {
	let mut rooms: Vec<OwnedRoomId> = services
		.rooms
		.metadata
		.iter_ids()
		.filter_map(|room_id| async move {
			if !services.rooms.metadata.is_banned(&room_id).await {
				Some(room_id.clone())
			} else {
				None
			}
		})
		.collect()
		.await;
	rooms.sort();
	Ok(rooms::list::unstable::Response::new(rooms))
}

/// # `GET /_continuwuity/admin/v1/rooms`
///
/// Lists rooms known to this server.
pub(crate) async fn list_rooms(
	State(services): State<crate::State>,
	body: Ruma<rooms::list::v1::Request>,
) -> Result<rooms::list::v1::Response> {
	let include_banned_rooms = body.include_banned_rooms;
	let rooms = services
		.rooms
		.metadata
		.iter_ids()
		.wide_filter_map(|room_id| async move {
			if include_banned_rooms || !services.rooms.metadata.is_banned(&room_id).await {
				Some(room_id.clone())
			} else {
				None
			}
		})
		.skip(body.offset.unwrap_or_default())
		.take(body.limit.unwrap_or(100).min(100))
		.broad_filter_map(|room_id| async move {
			let (
				banned,
				disabled,
				member_count,
				local_member_count,
				resident_server_count,
				published,
				create_event,
				encryption_event,
				name_event,
				topic_event,
				canonical_alias_event,
				join_rules_event,
				history_visibility_event,
				tombstone_event,
			) = join!(
				services.rooms.metadata.is_banned(&room_id),
				services.rooms.metadata.is_disabled(&room_id),
				services.rooms.state_cache.room_joined_count(&room_id),
				services
					.rooms
					.state_cache
					.active_local_users_in_room(&room_id)
					.count(),
				services.rooms.state_cache.room_servers(&room_id).count(),
				services.rooms.directory.is_public_room(&room_id),
				services.rooms.state_accessor.room_state_get(
					&room_id,
					&StateEventType::RoomCreate,
					""
				),
				services
					.rooms
					.state_accessor
					.room_state_get_content::<PossiblyRedactedRoomEncryptionEventContent>(
						&room_id,
						&StateEventType::RoomEncryption,
						""
					),
				services.rooms.state_accessor.room_state_get_content(
					&room_id,
					&StateEventType::RoomName,
					""
				),
				services.rooms.state_accessor.room_state_get_content(
					&room_id,
					&StateEventType::RoomTopic,
					""
				),
				services.rooms.state_accessor.room_state_get_content(
					&room_id,
					&StateEventType::RoomCanonicalAlias,
					""
				),
				services.rooms.state_accessor.room_state_get_content(
					&room_id,
					&StateEventType::RoomJoinRules,
					""
				),
				services.rooms.state_accessor.room_state_get_content(
					&room_id,
					&StateEventType::RoomHistoryVisibility,
					""
				),
				services
					.rooms
					.state_accessor
					.room_state_get_content::<PossiblyRedactedRoomTombstoneEventContent>(
						&room_id,
						&StateEventType::RoomTombstone,
						""
					),
			);
			let Ok(create_event) = create_event else {
				return None;
			};
			let create_content = create_event
				.get_content::<RoomCreateEventContent>()
				.expect("m.room.create content must be valid");
			Some(rooms::list::v1::MinimalRoomInfo {
				room_id,
				banned,
				disabled,
				member_count: usize::try_from(member_count.unwrap_or_default())
					.expect("u64 should fit in usize"),
				local_member_count,
				resident_server_count,
				creators: vec![create_event.sender],
				encrypted: encryption_event.is_ok_and(|c| c.algorithm.is_some()),
				federated: create_content.federate,
				published,
				version: create_content.room_version,
				name: name_event.unwrap_or(None),
				topic: topic_event.unwrap_or(None),
				canonical_alias: canonical_alias_event.unwrap_or(None),
				join_rules: join_rules_event.unwrap_or(None),
				history_visibility: history_visibility_event.unwrap_or(None),
				predecessor: create_content.predecessor.map(|c| c.room_id),
				successor: tombstone_event.map_or(None, |c| c.replacement_room),
			})
		})
		.collect()
		.await;
	Ok(rooms::list::v1::Response::new(rooms))
}
