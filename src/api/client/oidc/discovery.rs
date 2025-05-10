/// Manual implementation of [MSC2965]'s OIDC server discovery.
///
/// [MSC2965]: https://github.com/matrix-org/matrix-spec-proposals/pull/2965
use axum::extract::State;
use conduwuit::Result;
use ruma::{
	api::client::{
		discovery::get_authorization_server_metadata::{
			self,
			msc2965::{
				AccountManagementAction, AuthorizationServerMetadata, CodeChallengeMethod,
				GrantType, Prompt, Response, ResponseMode, ResponseType,
			},
		},
		error::{
			Error as ClientError, ErrorBody as ClientErrorBody, ErrorKind as ClientErrorKind,
		},
	},
	serde::Raw,
};

use crate::{Ruma, RumaResponse, conduwuit::Error};

/// # `GET /_matrix/client/unstable/org.matrix.msc2965/auth_metadata`
///
/// If `globals.auth.enable_oidc_login` is set, advertise this homeserver's
/// OAuth2 endpoints. Otherwise, MSC2965 requires that the homeserver responds
/// with 404/M_UNRECOGNIZED.
pub(crate) async fn get_auth_metadata(
	State(services): State<crate::State>,
	_body: Ruma<get_authorization_server_metadata::msc2965::Request>,
) -> Result<RumaResponse<Response>> {
	let unrecognized_error = Err(Error::Ruma(ClientError::new(
		http::StatusCode::NOT_FOUND,
		ClientErrorBody::Standard {
			kind: ClientErrorKind::Unrecognized,
			message: "This homeserver has disabled OIDC authentication.".to_owned(),
		},
	)));
	let Some(ref auth) = services.server.config.auth else {
		return unrecognized_error;
	};
	if !auth.enable_oidc_login {
		return unrecognized_error;
	}
	// Advertise this homeserver's access URL as the issuer URL.
	// Unwrap all Url::parse() calls because the issuer URL is validated at startup.
	let issuer = services.server.config.well_known.client.as_ref().unwrap();
	let account_management_uri = auth.enable_oidc_account_management.then_some(
		issuer
			.join("/_matrix/client/unstable/org.matrix.msc2964/account")
			.unwrap(),
	);

	let metadata = AuthorizationServerMetadata {
		issuer: issuer.clone(),
		authorization_endpoint: issuer
			.join("/_matrix/client/unstable/org.matrix.msc2964/authorize")
			.unwrap(),
		device_authorization_endpoint: Some(
			issuer
				.join("/_matrix/client/unstable/org.matrix.msc2964/device")
				.unwrap(),
		),
		token_endpoint: issuer
			.join("/_matrix/client/unstable/org.matrix.msc2964/token")
			.unwrap(),
		registration_endpoint: Some(
			issuer
				.join("/_matrix/client/unstable/org.matrix.msc2964/device/register")
				.unwrap(),
		),
		revocation_endpoint: issuer
			.join("/_matrix/client/unstable/org.matrix.msc2964/revoke")
			.unwrap(),
		response_types_supported: [ResponseType::Code].into(),
		grant_types_supported: [GrantType::AuthorizationCode, GrantType::RefreshToken].into(),
		response_modes_supported: [ResponseMode::Fragment, ResponseMode::Query].into(),
		code_challenge_methods_supported: [CodeChallengeMethod::S256].into(),
		account_management_uri,
		account_management_actions_supported: [
			AccountManagementAction::Profile,
			AccountManagementAction::SessionView,
			AccountManagementAction::SessionEnd,
		]
		.into(),
		prompt_values_supported: match services.server.config.allow_registration {
			| true => vec![Prompt::Create],
			| false => vec![],
		},
	};
	let metadata = Raw::new(&metadata).expect("authorization server metadata should serialize");

	Ok(RumaResponse(Response::new(metadata)))
}
