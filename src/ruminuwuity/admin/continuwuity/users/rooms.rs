pub mod v1 {
	use std::collections::BTreeMap;

	use ruma::{
		OwnedMxcUri, OwnedRoomAliasId, OwnedRoomId, OwnedUserId,
		api::{OAuthClientScope, auth_scheme::AccessToken, request, response},
		events::room::member::MembershipState,
		metadata,
	};
	use serde::{Deserialize, Serialize};

	metadata! {
		method: GET,
		rate_limited: false,
		authentication: AccessToken,
		history: {
			1.0 => "/_continuwuity/admin/v1/users/{user_id}/rooms",
		},
		required_client_scopes: [OAuthClientScope::ServerAdministration]
	}

	#[request]
	pub struct Request {
		#[ruma_api(path)]
		pub user_id: OwnedUserId,

		pub state: MembershipState,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub limit: Option<u32>,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub page: Option<u32>,
	}

	#[derive(Debug, Deserialize, Serialize, Default, Clone)]
	pub struct RoomInfo {
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub name: Option<String>,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub avatar_url: Option<OwnedMxcUri>,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		pub canonical_alias: Option<OwnedRoomAliasId>,
		pub member_count: u32,
	}

	#[response]
	pub struct Response {
		pub rooms: BTreeMap<OwnedRoomId, RoomInfo>,
	}

	impl Request {
		#[must_use]
		pub fn new(
			user_id: OwnedUserId,
			state: MembershipState,
			limit: Option<u32>,
			page: Option<u32>,
		) -> Self {
			Self { user_id, state, limit, page }
		}
	}

	impl Response {
		#[must_use]
		pub fn new(rooms: BTreeMap<OwnedRoomId, RoomInfo>) -> Self { Self { rooms } }
	}
}
