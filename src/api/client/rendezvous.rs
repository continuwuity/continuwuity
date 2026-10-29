use axum::extract::State;
use conduwuit::{Err, Result};
use http::HeaderMap;
use ruma::api::client::rendezvous::{
	create_rendezvous_session, delete_rendezvous_session, discover_rendezvous,
	get_rendezvous_session, update_rendezvous_session,
};
use service::Services;

use crate::{Ruma, router::ClientIdentity};

/// `GET /_matrix/client/unstable/io.element.msc4388/rendezvous`
pub(crate) async fn discover_rendezvous_route(
	State(services): State<crate::State>,
	body: Ruma<discover_rendezvous::unstable::Request>,
) -> Result<discover_rendezvous::unstable::Response> {
	Ok(discover_rendezvous::unstable::Response::new(rendezvous_available(
		&services,
		body.identity.as_ref(),
	)))
}

/// `POST /_matrix/client/unstable/io.element.msc4388/rendezvous/`
pub(crate) async fn create_rendezvous_session_route(
	State(services): State<crate::State>,
	body: Ruma<create_rendezvous_session::unstable_msc4388::Request>,
) -> Result<create_rendezvous_session::unstable_msc4388::Response> {
	if !rendezvous_available(&services, body.identity.as_ref()) {
		return Err!(Request(Forbidden("You are not authorized to create a rendezvous session")));
	}

	services.rendezvous.create_session(body.data.clone())
}

/// `PUT /_matrix/client/unstable/io.element.msc4388/rendezvous/{id}/{txn_id}`
pub(crate) async fn update_rendezvous_session_route(
	State(services): State<crate::State>,
	body: Ruma<update_rendezvous_session::unstable::Request>,
) -> Result<update_rendezvous_session::unstable::Response> {
	services.rendezvous.update_session(
		&body.id,
		body.txn_id.clone(),
		body.sequence_token.clone(),
		body.data.clone(),
	)
}

/// `GET /_matrix/client/unstable/io.element.msc4388/rendezvous/{id}`
pub(crate) async fn get_rendezvous_session_route(
	State(services): State<crate::State>,
	headers: HeaderMap,
	body: Ruma<get_rendezvous_session::unstable::Request>,
) -> Result<get_rendezvous_session::unstable::Response> {
	if let Some(mode) = headers.get("Sec-Fetch-Mode")
		&& let Ok(mode) = mode.to_str()
		&& mode == "navigate"
	{
		return Err!(Request(Forbidden(
			"Rendezvous sessions do not accept browser navigation requests."
		)));
	}

	services.rendezvous.get_session(&body.id)
}

/// `DELETE /_matrix/client/unstable/io.element.msc4388/rendezvous/{id}`
pub(crate) async fn delete_rendezvous_session_route(
	State(services): State<crate::State>,
	body: Ruma<delete_rendezvous_session::unstable::Request>,
) -> Result<delete_rendezvous_session::unstable::Response> {
	services.rendezvous.delete_session(&body.id)
}

fn rendezvous_available(services: &Services, identity: Option<&ClientIdentity>) -> bool {
	!services.config.require_auth_for_rendezvous || identity.is_some()
}
