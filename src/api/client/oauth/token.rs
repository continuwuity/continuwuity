use axum::{Form, Json, extract::State, response::IntoResponse};
use http::StatusCode;
use service::oauth::grant::{RevokeTokenRequest, TokenRequest};

pub(crate) async fn token_route(
	State(services): State<crate::State>,
	Form(request): Form<TokenRequest>,
) -> impl IntoResponse {
	match services.oauth.issue_token(request).await {
		| Ok(response) => Ok(Json(response)),
		| Err(err) => Err((StatusCode::BAD_REQUEST, err.message())),
	}
}

pub(crate) async fn revoke_token_route(
	State(services): State<crate::State>,
	Form(request): Form<RevokeTokenRequest>,
) -> impl IntoResponse {
	match services.oauth.revoke_token(request.token).await {
		| Ok(()) => Ok(StatusCode::OK),
		| Err(err) => Err((StatusCode::BAD_REQUEST, err.message())),
	}
}
