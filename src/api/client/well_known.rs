use axum::extract::State;
use conduwuit::{Err, Result};
use ruma::{
	api::client::discovery::{
		discover_homeserver::{self, HomeserverInfo, RtcFocusInfo},
		discover_support::{self, Contact, ContactRole},
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
			.effective_foci(&services.config.well_known.rtc_focus_server_urls)
			.into_iter()
			.map(|focus| RtcFocusInfo::new(focus.transport_type(), focus.data().into_owned()).unwrap())
			.collect()
	}))
}

/// # `GET /_matrix/client/v1/rtc/transports`
/// # `GET /_matrix/client/unstable/org.matrix.msc4143/rtc/transports`
///
/// Returns the list of MatrixRTC foci (transports) configured for this
/// homeserver, implementing MSC4143.
pub(crate) async fn get_rtc_transports(
	State(services): State<crate::State>,
	_body: Ruma<ruma::api::client::rtc::transports::v1::Request>,
) -> Result<ruma::api::client::rtc::transports::v1::Response> {
	Ok(ruma::api::client::rtc::transports::v1::Response::new(
		services
			.config
			.matrix_rtc
			.effective_foci(&services.config.well_known.rtc_focus_server_urls),
	))
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
	let pgp_key = services.config.well_known.support_pgp_key.clone();

	// TODO: support defining multiple contacts in the config
	let mut contacts: Vec<Contact> = vec![];

	let role = services
		.config
		.well_known
		.support_role
		.clone()
		.unwrap_or(ContactRole::Admin);

	// Add configured contact if at least one contact method is specified
	let configured_contact = match (matrix_id, email_address) {
		| (Some(matrix_id), email_address) =>
			Some(assign!(Contact::with_matrix_id(role, matrix_id), { email_address })),
		| (None, Some(email_address)) => Some(Contact::with_email_address(role, email_address)),
		| (None, None) => None,
	};

	if let Some(mut configured_contact) = configured_contact {
		configured_contact.pgp_key = pgp_key;

		contacts.push(configured_contact);
	}

	// Try to add admin users as contacts if no contacts are configured
	if contacts.is_empty() {
		let admin_users = services.admin.get_admins().await;

		for user_id in &admin_users {
			if *user_id == services.globals.server_user {
				continue;
			}

			contacts.push(Contact::with_matrix_id(ContactRole::Admin, user_id.to_owned()));
		}
	}

	if contacts.is_empty() && support_page.is_none() {
		// No admin room, no configured contacts, and no support page
		return Err!(Request(NotFound("No support information is available.")));
	}

	Ok(assign!(discover_support::Response::with_contacts(contacts), { support_page }))
}
