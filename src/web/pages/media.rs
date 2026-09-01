use axum::{
	Router,
	extract::{Path, State},
	http::{self, HeaderMap},
	routing::get,
};
use conduwuit_core::result::FlatOk;
use conduwuit_service::media::mxc::Mxc;
use ruma::{OwnedUserId, profile::ProfileFieldName};
use serde::{Deserialize, Serialize};

use crate::{WebError, extract::Expect, pages::Result, response, session::User};

pub(crate) fn build() -> Router<crate::State> {
	Router::new().route("/profile/{user_id}/{field}", get(get_profile_media))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProfileMediaPath {
	user_id: OwnedUserId,
	field: ProfileFieldName,
}

async fn get_profile_media(
	State(services): State<crate::State>,
	Expect(Path(ProfileMediaPath { user_id, field })): Expect<Path<ProfileMediaPath>>,
	headers: HeaderMap,
	requesting_user: User<true>,
) -> Result {
	if requesting_user
		.into_session()
		.map(|session| session.user_id)
		.as_ref()
		!= Some(&user_id)
	{
		return Err(WebError::Forbidden(
			"You may not access this user's profile media.".to_owned(),
		));
	}

	let Some(field) = services
		.users
		.get_local_profile_field(&user_id, field)
		.await
	else {
		return Err(WebError::NotFound);
	};

	let value = field.value();
	let Some(Ok(mxc)) = value.as_str().map(Mxc::try_from) else {
		return Err(WebError::BadRequest("Profile field value is not a MXC URI".to_owned()));
	};

	if let Some(Ok(etag)) = headers
		.get(http::header::IF_NONE_MATCH)
		.and_then(|v| v.to_str().ok())
		.map(Mxc::try_from)
		&& mxc == etag
	{
		return response!(http::StatusCode::NOT_MODIFIED);
	}

	let Some(media) = services.media.get(&mxc).await.flat_ok() else {
		return Err(WebError::NotFound);
	};

	let content = media.content.expect("media should have content");

	response!((
		[
			(http::header::CONTENT_TYPE, media.content_type.unwrap_or_default()),
			(http::header::CONTENT_LENGTH, content.len().to_string()),
			(http::header::CACHE_CONTROL, "max-age=31536000".to_owned()),
			(http::header::ETAG, mxc.to_string()),
		],
		content
	))
}
