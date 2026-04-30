use axum::{Form, Json, extract::State, response::IntoResponse};
use http::StatusCode;
use service::oauth::grant::TokenRequest;

pub(crate) async fn token_route(
	State(services): State<crate::State>,
	Form(request): Form<TokenRequest>,
) -> impl IntoResponse {
	match services.oauth.issue_token(request).await {
		| Ok(response) => Ok(Json(response).into_response()),
		| Err(err) => Err((StatusCode::BAD_REQUEST, err.message())),
	}
}
