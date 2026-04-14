use axum::{
	Json,
	extract::State,
	response::{IntoResponse, Response},
};
use http::StatusCode;
use serde::Serialize;
use service::oauth::client_metadata::ClientMetadata;

#[derive(Serialize)]
struct RegisteredClient {
	client_id: String,
	#[serde(flatten)]
	metadata: ClientMetadata,
}

pub(crate) async fn register_client_route(
	State(services): State<crate::State>,
	Json(metadata): Json<ClientMetadata>,
) -> Result<Response, Response> {
	let client_id = services
		.oauth
		.register_client(&metadata)
		.await
		.map_err(|err| (StatusCode::BAD_REQUEST, err.to_owned()).into_response())?;

	Ok(Json(RegisteredClient { client_id, metadata }).into_response())
}
