use axum::{Form, Json, extract::State, response::IntoResponse};
use http::StatusCode;
use service::oauth::grant::DeviceCodeRequest;

pub(crate) async fn device_authorization_route(
	State(services): State<crate::State>,
	Form(request): Form<DeviceCodeRequest>,
) -> impl IntoResponse {
	match services.oauth.request_device_code(request).await {
		| Ok(response) => Ok(Json(response)),
		| Err(err) => Err((StatusCode::BAD_REQUEST, Json(err))),
	}
}
