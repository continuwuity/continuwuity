use axum::extract::State;
use conduwuit::{Err, Result};
use ruma::{
	api::client::{
		discovery::{
			discover_homeserver::{self, HomeserverInfo},
			discover_policy_server, discover_support,
		},
		rtc,
	},
	assign,
};

use crate::Ruma;

/// # `GET /.well-known/matrix/client`
///
/// Returns the .well-known URL if it is configured, otherwise returns 404.
pub(crate) async fn well_known_client(
	State(services): State<crate::State>,
	_body: Ruma<discover_homeserver::Request>,
) -> Result<discover_homeserver::Response> {
	let client_url = match services.config.well_known.client.as_ref() {
		| Some(url) => url.to_string(),
		| None =>
			return Err!(Request(NotFound(
				"This server is not configured to serve well-known client information."
			))),
	};

	Ok(assign!(discover_homeserver::Response::new(HomeserverInfo::new(client_url)), {
		identity_server: None,
		tile_server: None,
		rtc_foci: services
			.config
			.matrix_rtc
			.foci
			.clone()
	}))
}

/// # `GET /_matrix/client/v1/rtc/transports`
///
/// Returns the list of MatrixRTC transports configured for this homeserver,
/// implementing the stable version of MSC4143.
pub(crate) async fn get_rtc_transports(
	State(services): State<crate::State>,
	_body: Ruma<rtc::transports::v1::Request>,
) -> Result<rtc::transports::v1::Response> {
	Ok(rtc::transports::v1::Response::new(services.config.matrix_rtc.foci.clone()))
}

/// # `GET /_matrix/client/unstable/org.matrix.msc4143/rtc/transports`
///
/// Unstable version of [`get_rtc_transports`], which names the response field
/// `rtc_transports` instead of `transports`.
pub(crate) async fn get_rtc_transports_unstable(
	State(services): State<crate::State>,
	_body: Ruma<rtc::transports::msc4143::Request>,
) -> Result<rtc::transports::msc4143::Response> {
	Ok(rtc::transports::msc4143::Response::new(services.config.matrix_rtc.foci.clone()))
}

/// # `GET /.well-known/matrix/support`
///
/// Server support contact and support page of a homeserver's domain.
/// Implements MSC1929 for server discovery.
/// If no configuration is set, uses admin users as contacts.
pub(crate) async fn well_known_support(
	State(services): State<crate::State>,
	_body: Ruma<discover_support::Request>,
) -> Result<discover_support::Response> {
	let support_page = services
		.config
		.well_known
		.support_page
		.as_ref()
		.map(ToString::to_string);

	let contacts = services.admin.get_support_contacts().await;

	if contacts.is_empty() && support_page.is_none() {
		// No admin room, no configured contacts, and no support page
		return Err!(Request(NotFound("No support information is available.")));
	}

	Ok(assign!(discover_support::Response::with_contacts(contacts), { support_page }))
}

/// # `GET /.well-known/matrix/policy_server`
///
/// Advertises the policy server's public key, allowing clients to discover the
/// values to be set in m.room.policy. Introduced in spec v1.18.
pub(crate) async fn well_known_policy_server(
	State(services): State<crate::State>,
	_body: Ruma<discover_policy_server::Request>,
) -> Result<discover_policy_server::Response> {
	if let Some(key) = services.config.well_known.policy_server_public_key.clone() {
		Ok(discover_policy_server::Response::new(key))
	} else {
		Err!(Request(NotFound("No policy server available.")))
	}
}
