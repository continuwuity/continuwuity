use std::collections::BTreeMap;

use axum::extract::State;
use conduwuit::{Result, Server};
use ruma::{
	RoomVersionId,
	api::client::discovery::get_capabilities::{
		self,
		v3::{
			Capabilities, GetLoginTokenCapability, RoomVersionStability, RoomVersionsCapability,
			ThirdPartyIdChangesCapability,
		},
	},
};

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
	capabilities.room_versions = RoomVersionsCapability::new(
		services.server.config.default_room_version.clone(),
		available,
	);

	// Only allow 3pid changes if SMTP is configured
	capabilities.thirdparty_id_changes =
		ThirdPartyIdChangesCapability::new(services.threepid.email_requirement().may_change());

	capabilities.get_login_token =
		GetLoginTokenCapability::new(services.server.config.login_via_existing_session);

	capabilities.forget_forced_upon_leave.enabled = true;

	if services
		.users
		.is_admin(body.identity.expect_sender_user()?)
		.await
	{
		capabilities.account_moderation.lock = true;
		capabilities.account_moderation.suspend = true
	}

	Ok(get_capabilities::v3::Response::new(capabilities))
}
