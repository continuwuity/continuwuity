use std::time::Duration;

use axum::extract::State;
use conduwuit::{Error, Result, utils};
use ruma::{
	api::client::{account, error::ErrorKind},
	authentication::TokenType,
};

use super::TOKEN_LENGTH;
use crate::Ruma;

/// # `POST /_matrix/client/v3/user/{userId}/openid/request_token`
///
/// Request an OpenID token to verify identity with third-party services.
///
/// - The token generated is only valid for the OpenID API
pub(crate) async fn create_openid_token_route(
	State(services): State<crate::State>,
	body: Ruma<account::request_openid_token::v3::Request>,
) -> Result<account::request_openid_token::v3::Response> {
	let sender_user = body.sender_user();

	if sender_user != body.user_id {
		return Err(Error::BadRequest(
			ErrorKind::InvalidParam,
			"Not allowed to request OpenID tokens on behalf of other users",
		));
	}

	let access_token = utils::random_string(TOKEN_LENGTH);

	let expires_in = services
		.users
		.create_openid_token(&body.user_id, &access_token)?;

	Ok(account::request_openid_token::v3::Response {
		access_token,
		token_type: TokenType::Bearer,
		matrix_server_name: services.server.name.clone(),
		expires_in: Duration::from_secs(expires_in),
	})
}
