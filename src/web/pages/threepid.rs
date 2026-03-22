use axum::{
	Router,
	extract::{Query, State, rejection::QueryRejection},
	response::IntoResponse,
	routing::get,
};
use ruma::OwnedSessionId;
use serde::Deserialize;

use crate::{WebError, template};

template! {
	struct ThreepidValidation use "threepid_validation.html.j2" {}
}

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/3pid/email/validate", get(threepid_validation))
}

#[derive(Deserialize)]
struct ThreepidValidationQuery {
	session: OwnedSessionId,
	token: String,
}

async fn threepid_validation(
	State(services): State<crate::State>,
	query: Result<Query<ThreepidValidationQuery>, QueryRejection>,
) -> Result<impl IntoResponse, WebError> {
	let Query(query) = query?;

	services
		.threepid
		.try_validate_session(&query.session, &query.token)
		.await
		.map_err(|message| WebError::BadRequest(message.into_owned()))?;

	Ok(ThreepidValidation::new(&services))
}
