use axum::{Json, extract::State, response::IntoResponse};
use conduwuit::{
	Result,
	matrix::versions::{unstable_features, versions},
};
use futures::StreamExt;
use ruma::{api::client::discovery::get_supported_versions, assign};

use crate::Ruma;

/// # `GET /_matrix/client/versions`
///
/// Get the versions of the specification and unstable features supported by
/// this server.
///
/// - Versions take the form MAJOR.MINOR.PATCH
/// - Only the latest PATCH release will be reported for each MAJOR.MINOR value
/// - Unstable features are namespaced and may include version information in
///   their name
///
/// Note: Unstable features are used while developing new features. Clients
/// should avoid using unstable features in their stable releases
pub(crate) async fn get_supported_versions_route(
	_body: Ruma<get_supported_versions::Request>,
) -> Result<get_supported_versions::Response> {
	Ok(assign!(
		get_supported_versions::Response::new(versions()),
		{ unstable_features: unstable_features() }
	))
}

/// # `GET /_conduwuit/server_version`
///
/// Conduwuit-specific API to get the server version, results akin to
/// `/_matrix/federation/v1/version`
pub(crate) async fn conduwuit_server_version() -> Result<impl IntoResponse> {
	Ok(Json(serde_json::json!({
		"name": conduwuit::version::name(),
		"version": conduwuit::version::version(),
	})))
}

/// # `GET /_conduwuit/local_user_count`
///
/// conduwuit-specific API to return the amount of users registered on this
/// homeserver. Endpoint is disabled if federation is disabled for privacy. This
/// only includes active users (not deactivated, etc)
pub(crate) async fn conduwuit_local_user_count(
	State(services): State<crate::State>,
) -> Result<impl IntoResponse> {
	let user_count = services.users.list_local_users().count().await;

	Ok(Json(serde_json::json!({
		"count": user_count
	})))
}
