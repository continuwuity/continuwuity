use axum::{Json, extract::State, response::IntoResponse};
use conduwuit::{Error, Result};
use ruma::api::client::{
	discovery::{
		discover_homeserver::{self, HomeserverInfo, SlidingSyncProxyInfo},
		discover_support::{self, Contact},
	},
	error::ErrorKind,
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
		| None => return Err(Error::BadRequest(ErrorKind::NotFound, "Not found.")),
	};

	Ok(discover_homeserver::Response {
		homeserver: HomeserverInfo { base_url: client_url.clone() },
		identity_server: None,
		sliding_sync_proxy: Some(SlidingSyncProxyInfo { url: client_url }),
		tile_server: None,
		rtc_foci: vec![],
	})
}

/// # `GET /_matrix/client/v1/rtc/transports`
/// # `GET /_matrix/client/unstable/org.matrix.msc4143/rtc/transports`
///
/// Returns the list of MatrixRTC foci (transports) configured for this
/// homeserver, implementing MSC4143.
pub(crate) async fn get_rtc_transports(
	State(services): State<crate::State>,
) -> Result<impl IntoResponse> {
	Ok(Json(serde_json::json!({
		"rtc_foci": services.config.matrix_rtc.foci,
	})))
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

	let email_address = services.config.well_known.support_email.clone();
	let matrix_id = services.config.well_known.support_mxid.clone();

	// TODO: support defining multiple contacts in the config
	let mut contacts: Vec<Contact> = vec![];

	let role_value = services
		.config
		.well_known
		.support_role
		.clone()
		.unwrap_or_else(|| "m.role.admin".to_owned().into());

	// Add configured contact if at least one contact method is specified
	if email_address.is_some() || matrix_id.is_some() {
		contacts.push(Contact {
			role: role_value.clone(),
			email_address: email_address.clone(),
			matrix_id: matrix_id.clone(),
		});
	}

	// Try to add admin users as contacts if no contacts are configured
	if contacts.is_empty() {
		let admin_users = services.admin.get_admins().await;

		for user_id in &admin_users {
			if *user_id == services.globals.server_user {
				continue;
			}

			contacts.push(Contact {
				role: role_value.clone(),
				email_address: None,
				matrix_id: Some(user_id.to_owned()),
			});
		}
	}

	if contacts.is_empty() && support_page.is_none() {
		// No admin room, no configured contacts, and no support page
		return Err(Error::BadRequest(ErrorKind::NotFound, "Not found."));
	}

	Ok(discover_support::Response { contacts, support_page })
}

/// # `GET /client/server.json`
///
/// Endpoint provided by sliding sync proxy used by some clients such as Element
/// Web as a non-standard health check.
pub(crate) async fn syncv3_client_server_json(
	State(services): State<crate::State>,
) -> Result<impl IntoResponse> {
	let server_url = match services.config.well_known.client.as_ref() {
		| Some(url) => url.to_string(),
		| None => match services.config.well_known.server.as_ref() {
			| Some(url) => url.to_string(),
			| None => return Err(Error::BadRequest(ErrorKind::NotFound, "Not found.")),
		},
	};

	Ok(Json(serde_json::json!({
		"server": server_url,
		"version": conduwuit::version(),
	})))
}
