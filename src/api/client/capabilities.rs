use std::collections::BTreeMap;

use axum::extract::State;
use conduwuit::{Result, Server};
use ruma::{
	RoomVersionId,
	api::client::discovery::get_capabilities::{
		self, Capabilities, GetLoginTokenCapability, RoomVersionStability,
		RoomVersionsCapability, ThirdPartyIdChangesCapability,
	},
};
use serde_json::json;

use crate::Ruma;

/// # `GET /_matrix/client/v3/capabilities`
///
/// Get information on the supported feature set and other relevant capabilities
/// of this server.
pub(crate) async fn get_capabilities_route(
	State(services): State<crate::State>,
	body: Ruma<get_capabilities::v3::Request>,
) -> Result<get_capabilities::v3::Response> {
	let available: BTreeMap<RoomVersionId, RoomVersionStability> =
		Server::available_room_versions().collect();

	let mut capabilities = Capabilities::default();
	capabilities.room_versions = RoomVersionsCapability {
		available,
		default: services.server.config.default_room_version.clone(),
	};

	// we do not implement 3PID stuff
	capabilities.thirdparty_id_changes = ThirdPartyIdChangesCapability { enabled: false };

	capabilities.get_login_token = GetLoginTokenCapability {
		enabled: services.server.config.login_via_existing_session,
	};

	// MSC4133 capability
	capabilities.set("uk.tcpip.msc4133.profile_fields", json!({"enabled": true}))?;

	capabilities.set(
		"org.matrix.msc4267.forget_forced_upon_leave",
		json!({"enabled": services.config.forget_forced_upon_leave}),
	)?;

	if services
		.users
		.is_admin(body.sender_user.as_ref().unwrap())
		.await
	{
		// Advertise suspension API
		capabilities.set("uk.timedout.msc4323", json!({"suspend":true, "lock": false}))?;
	}

	Ok(get_capabilities::v3::Response { capabilities })
}
