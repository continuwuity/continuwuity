pub mod unstable {
	use ruma::{
		OwnedRoomId,
		api::{auth_scheme::AccessToken, request, response},
		metadata,
	};

	metadata! {
		method: GET,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			unstable => "/_continuwuity/admin/rooms/list",
		}
	}

	#[request]
	#[derive(Default)]
	pub struct Request;

	#[response]
	pub struct Response {
		/// A list of room IDs known to this server.
		pub rooms: Vec<OwnedRoomId>,
	}

	impl Request {
		#[must_use]
		pub fn new() -> Self { Self::default() }
	}

	impl Response {
		#[must_use]
		pub fn new(rooms: Vec<OwnedRoomId>) -> Self { Self { rooms } }
	}
}

pub mod v1 {
	use ruma::{
		OwnedRoomId, OwnedUserId, RoomVersionId,
		api::{auth_scheme::AccessToken, request, response},
		events::room::{
			canonical_alias::PossiblyRedactedRoomCanonicalAliasEventContent,
			history_visibility::PossiblyRedactedRoomHistoryVisibilityEventContent,
			join_rules::PossiblyRedactedRoomJoinRulesEventContent,
			name::PossiblyRedactedRoomNameEventContent,
			topic::PossiblyRedactedRoomTopicEventContent,
		},
		metadata,
		serde::{default_true, is_default},
	};

	metadata! {
		method: GET,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_continuwuity/admin/v1/rooms",
		}
	}

	#[request]
	#[derive(Default)]
	pub struct Request {
		/// The maximum number of results to return in this page. Maximum (and
		/// default) is 100.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "is_default")]
		pub limit: Option<usize>,

		/// The number of results to skip over before returning results. Default
		/// is 0.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "is_default")]
		pub offset: Option<usize>,

		/// If true, includes banned rooms in the response.
		#[ruma_api(query)]
		#[serde(default, skip_serializing_if = "is_default")]
		pub include_banned_rooms: bool,
	}

	#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
	pub struct MinimalRoomInfo {
		/// The room's unique ID.
		pub room_id: OwnedRoomId,
		/// If true, this room is banned, and cannot be joined by non-admins.
		#[serde(default, skip_serializing_if = "is_default")]
		pub banned: bool,
		/// If true, this room has federation disabled, but can still be locally
		/// used.
		#[serde(default, skip_serializing_if = "is_default")]
		pub disabled: bool,
		/// The total number of joined members in this room.
		#[serde(default, skip_serializing_if = "is_default")]
		pub member_count: usize,
		/// The total number of joined members in this room that are local to
		/// this server.
		#[serde(default, skip_serializing_if = "is_default")]
		pub local_member_count: usize,
		/// The number of unique homeservers currently joined to this room.
		#[serde(default, skip_serializing_if = "is_default")]
		pub resident_server_count: usize,
		/// The users who created this room.
		///
		/// The first entry is always the sender of the `m.room.create` event.
		/// Any entries thereafter are additional creators in v12+ rooms. An
		/// empty vec indicates the room is not known.
		#[serde(default, skip_serializing_if = "is_default")]
		pub creators: Vec<OwnedUserId>,
		/// If true, this room has encryption enabled.
		#[serde(default, skip_serializing_if = "is_default")]
		pub encrypted: bool,
		/// If true, this room is allowed to be federated (`m.federate` is not
		/// `false` in `m.room.create`).
		#[serde(default = "default_true", skip_serializing_if = "is_default")]
		pub federated: bool,
		/// If true, this room is published to this server's room directory.
		#[serde(default, skip_serializing_if = "is_default")]
		pub published: bool,
		/// The version of the room.
		pub version: RoomVersionId,
		/// The event content for the `m.room.name` event, if any is present.
		/// May be redacted.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub name: Option<PossiblyRedactedRoomNameEventContent>,
		/// The event content for the `m.room.topic` event, if any is present.
		/// May be redacted.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub topic: Option<PossiblyRedactedRoomTopicEventContent>,
		/// The event content for the `m.room.canonical_alias` event, if any is
		/// present. May be redacted.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub canonical_alias: Option<PossiblyRedactedRoomCanonicalAliasEventContent>,
		/// The event content for the `m.room.join_rules` event, if any is
		/// present. May be redacted.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub join_rules: Option<PossiblyRedactedRoomJoinRulesEventContent>,
		/// The event content for the `m.room.history_visibility` event, if any
		/// is present. May be redacted.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub history_visibility: Option<PossiblyRedactedRoomHistoryVisibilityEventContent>,
		/// The ID of the room which replaces this one, if any.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub successor: Option<OwnedRoomId>,
		/// The ID of the room which preceded this one, if any.
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub predecessor: Option<OwnedRoomId>,
	}

	#[response]
	pub struct Response {
		/// A list of rooms known to this server.
		pub rooms: Vec<MinimalRoomInfo>,
	}

	impl Request {
		#[must_use]
		pub fn new() -> Self { Self::default() }
	}

	impl Response {
		#[must_use]
		pub fn new(rooms: Vec<MinimalRoomInfo>) -> Self { Self { rooms } }
	}
}
