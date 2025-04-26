use axum::extract::State;
use conduwuit::{Result, err};
use ruma::api::client::filter::{create_filter, get_filter};

use crate::Ruma;

/// # `GET /_matrix/client/r0/user/{userId}/filter/{filterId}`
///
/// Loads a filter that was previously created.
///
/// - A user can only access their own filters
pub(crate) async fn get_filter_route(
	State(services): State<crate::State>,
	body: Ruma<get_filter::v3::Request>,
) -> Result<get_filter::v3::Response> {
	services
		.users
		.get_filter(body.sender_user(), &body.filter_id)
		.await
		.map(get_filter::v3::Response::new)
		.map_err(|_| err!(Request(NotFound("Filter not found."))))
}

/// # `PUT /_matrix/client/r0/user/{userId}/filter`
///
/// Creates a new filter to be used by other endpoints.
pub(crate) async fn create_filter_route(
	State(services): State<crate::State>,
	body: Ruma<create_filter::v3::Request>,
) -> Result<create_filter::v3::Response> {
	let filter_id = services
		.users
		.create_filter(body.sender_user(), &body.filter);

	Ok(create_filter::v3::Response::new(filter_id))
}
