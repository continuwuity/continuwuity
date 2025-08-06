use std::time::Duration;

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Error, Result, debug, debug_info, err, error,
	utils::{self, content_disposition::make_content_disposition, math::ruma_from_usize},
	warn,
};
use conduwuit_service::{
	Services,
	media::{CACHE_CONTROL_IMMUTABLE, CORP_CROSS_ORIGIN, Dim, FileMeta, MXC_LENGTH},
};
use http::StatusCode;
use reqwest::Url;
use ruma::{
	Mxc, UserId,
	api::client::{
		authenticated_media::{
			get_content, get_content_as_filename, get_content_thumbnail, get_media_config,
			get_media_preview,
		},
		error::ErrorKind,
		media::{create_content, create_content_async, create_mxc_uri},
	},
};

use crate::Ruma;

/// # `GET /_matrix/client/v1/media/config`
pub(crate) async fn get_media_config_route(
	State(services): State<crate::State>,
	_body: Ruma<get_media_config::v1::Request>,
) -> Result<get_media_config::v1::Response> {
	Ok(get_media_config::v1::Response {
		upload_size: ruma_from_usize(services.server.config.max_request_size),
	})
}

/// # `POST /_matrix/media/v3/upload`
///
/// Permanently save media in the server.
///
/// - Some metadata will be saved in the database
/// - Media will be saved in the media/ directory
#[tracing::instrument(
	name = "media_upload",
	level = "debug",
	skip_all,
	fields(%client),
)]
pub(crate) async fn create_content_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<create_content::v3::Request>,
) -> Result<create_content::v3::Response> {
	let user = body.sender_user();
	if services.users.is_suspended(user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}

	let filename = body.filename.as_deref();
	let content_type = body.content_type.as_deref();
	let content_disposition = make_content_disposition(None, content_type, filename);
	let ref mxc = Mxc {
		server_name: services.globals.server_name(),
		media_id: &utils::random_string(MXC_LENGTH),
	};

	services
		.media
		.create(mxc, Some(user), Some(&content_disposition), content_type, &body.file)
		.await?;

	let blurhash = body.generate_blurhash.then(|| {
		services
			.media
			.create_blurhash(&body.file, content_type, filename)
			.ok()
			.flatten()
	});

	Ok(create_content::v3::Response {
		content_uri: mxc.to_string().into(),
		blurhash: blurhash.flatten(),
	})
}

/// # `POST /_matrix/media/v1/create`
///
/// Creates a new MXC URI to later be populated.
#[tracing::instrument(
	name = "media_create_async",
	level = "debug",
	skip_all,
	fields(%client),
)]
pub(crate) async fn create_async_mxc_uri_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<create_mxc_uri::v1::Request>,
) -> Result<create_mxc_uri::v1::Response> {
	let user = body.sender_user();
	if services.users.is_suspended(user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}
	let ref mxc = Mxc {
		server_name: services.globals.server_name(),
		media_id: &utils::random_string(MXC_LENGTH),
	};
	services.media.create_async(mxc, Some(user)).await?;
	// TODO: add expiring MXC URIs to prevent exhaustion of MXC IDs
	Ok(create_mxc_uri::v1::Response {
		content_uri: mxc.to_string().into(),
		unused_expires_at: None,
	})
}

/// # `PUT /_matrix/media/v3/upload/{serverName}/{mediaId}`
///
/// Permanently save media in the server, using an existing MXC URI.
#[tracing::instrument(
	name = "media_async_upload",
	level = "debug",
	skip_all,
	fields(%client),
)]
pub(crate) async fn upload_async_media_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<create_content_async::v3::Request>,
) -> Result<create_content_async::v3::Response> {
	let user = body.sender_user();
	if services.users.is_suspended(user).await? {
		return Err!(Request(UserSuspended("You cannot perform this action while suspended.")));
	}
	let ref mxc = Mxc {
		server_name: &body.server_name,
		media_id: &body.media_id,
	};
	if !services.globals.server_is_ours(&body.server_name) {
		return Err!(Request(Forbidden("Media uri does not belong to us.")));
	}
	if !services.media.exists(mxc).await {
		return Err!(Request(NotFound("Media uri does not exist.")));
	} else if services.media.is_populated(mxc).await {
		return Err(Error::Request(
			ErrorKind::CannotOverwriteMedia,
			"Media uri is already populated.".into(),
			StatusCode::CONFLICT,
		));
	}

	let filename = body.filename.as_deref();
	let content_type = body.content_type.as_deref();
	let content_disposition = make_content_disposition(None, content_type, filename);

	services
		.media
		.create(mxc, Some(user), Some(&content_disposition), content_type, &body.file)
		.await?;

	Ok(create_content_async::v3::Response {})
}

/// # `GET /_matrix/client/v1/media/thumbnail/{serverName}/{mediaId}`
///
/// Load media thumbnail from our server or over federation.
#[tracing::instrument(
	name = "media_thumbnail_get",
	level = "debug",
	skip_all,
	fields(%client),
)]
pub(crate) async fn get_content_thumbnail_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_content_thumbnail::v1::Request>,
) -> Result<get_content_thumbnail::v1::Response> {
	let user = body.sender_user();

	let dim = Dim::from_ruma(body.width, body.height, body.method.clone())?;
	let mxc = Mxc {
		server_name: &body.server_name,
		media_id: &body.media_id,
	};

	let FileMeta {
		content,
		content_type,
		content_disposition,
	} = fetch_thumbnail(&services, &mxc, user, body.timeout_ms, &dim).await?;

	Ok(get_content_thumbnail::v1::Response {
		file: content.expect("entire file contents"),
		content_type: content_type.map(Into::into),
		cross_origin_resource_policy: Some(CORP_CROSS_ORIGIN.into()),
		cache_control: Some(CACHE_CONTROL_IMMUTABLE.into()),
		content_disposition,
	})
}

/// # `GET /_matrix/client/v1/media/download/{serverName}/{mediaId}`
///
/// Load media from our server or over federation.
#[tracing::instrument(
	name = "media_get",
	level = "debug",
	skip_all,
	fields(%client),
)]
pub(crate) async fn get_content_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_content::v1::Request>,
) -> Result<get_content::v1::Response> {
	let user = body.sender_user();

	let mxc = Mxc {
		server_name: &body.server_name,
		media_id: &body.media_id,
	};

	let FileMeta {
		content,
		content_type,
		content_disposition,
	} = fetch_file(&services, &mxc, user, body.timeout_ms, None).await?;

	Ok(get_content::v1::Response {
		file: content.expect("entire file contents"),
		content_type: content_type.map(Into::into),
		cross_origin_resource_policy: Some(CORP_CROSS_ORIGIN.into()),
		cache_control: Some(CACHE_CONTROL_IMMUTABLE.into()),
		content_disposition,
	})
}

/// # `GET /_matrix/client/v1/media/download/{serverName}/{mediaId}/{fileName}`
///
/// Load media from our server or over federation as fileName.
#[tracing::instrument(
	name = "media_get_af",
	level = "debug",
	skip_all,
	fields(%client),
)]
pub(crate) async fn get_content_as_filename_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_content_as_filename::v1::Request>,
) -> Result<get_content_as_filename::v1::Response> {
	let user = body.sender_user();

	let mxc = Mxc {
		server_name: &body.server_name,
		media_id: &body.media_id,
	};

	let FileMeta {
		content,
		content_type,
		content_disposition,
	} = fetch_file(&services, &mxc, user, body.timeout_ms, Some(&body.filename)).await?;

	Ok(get_content_as_filename::v1::Response {
		file: content.expect("entire file contents"),
		content_type: content_type.map(Into::into),
		cross_origin_resource_policy: Some(CORP_CROSS_ORIGIN.into()),
		cache_control: Some(CACHE_CONTROL_IMMUTABLE.into()),
		content_disposition,
	})
}

/// # `GET /_matrix/client/v1/media/preview_url`
///
/// Returns URL preview.
#[tracing::instrument(
	name = "url_preview",
	level = "debug",
	skip_all,
	fields(%client),
)]
pub(crate) async fn get_media_preview_route(
	State(services): State<crate::State>,
	InsecureClientIp(client): InsecureClientIp,
	body: Ruma<get_media_preview::v1::Request>,
) -> Result<get_media_preview::v1::Response> {
	let sender_user = body.sender_user();

	let url = &body.url;
	let url = Url::parse(&body.url).map_err(|e| {
		err!(Request(InvalidParam(
			debug_warn!(%sender_user, %url, "Requested URL is not valid: {e}")
		)))
	})?;

	if !services.media.url_preview_allowed(&url) {
		return Err!(Request(Forbidden(
			debug_warn!(%sender_user, %url, "URL is not allowed to be previewed")
		)));
	}

	let preview = services
		.media
		.get_url_preview(&url)
		.await
		.map_err(|error| {
			err!(Request(Unknown(
				debug_error!(%sender_user, %url, "Failed to fetch URL preview: {error}")
			)))
		})?;

	serde_json::value::to_raw_value(&preview)
		.map(get_media_preview::v1::Response::from_raw_value)
		.map_err(|error| {
			err!(Request(Unknown(
				debug_error!(%sender_user, %url, "Failed to parse URL preview: {error}")
			)))
		})
}

async fn fetch_thumbnail(
	services: &Services,
	mxc: &Mxc<'_>,
	user: &UserId,
	timeout_ms: Duration,
	dim: &Dim,
) -> Result<FileMeta> {
	let FileMeta {
		content,
		content_type,
		content_disposition,
	} = fetch_thumbnail_meta(services, mxc, user, timeout_ms, dim).await?;

	let content_disposition = Some(make_content_disposition(
		content_disposition.as_ref(),
		content_type.as_deref(),
		None,
	));

	Ok(FileMeta {
		content,
		content_type,
		content_disposition,
	})
}

async fn fetch_file(
	services: &Services,
	mxc: &Mxc<'_>,
	user: &UserId,
	timeout_ms: Duration,
	filename: Option<&str>,
) -> Result<FileMeta> {
	let FileMeta {
		content,
		content_type,
		content_disposition,
	} = fetch_file_meta(services, mxc, user, timeout_ms).await?;

	let content_disposition = Some(make_content_disposition(
		content_disposition.as_ref(),
		content_type.as_deref(),
		filename,
	));

	Ok(FileMeta {
		content,
		content_type,
		content_disposition,
	})
}

async fn fetch_thumbnail_meta(
	services: &Services,
	mxc: &Mxc<'_>,
	user: &UserId,
	timeout_ms: Duration,
	dim: &Dim,
) -> Result<FileMeta> {
	if let Some(filemeta) = services.media.get_thumbnail(mxc, dim).await? {
		return Ok(filemeta);
	}

	if services.globals.server_is_ours(mxc.server_name) {
		return Err!(Request(NotFound("Local thumbnail not found.")));
	}

	services
		.media
		.fetch_remote_thumbnail(mxc, Some(user), None, timeout_ms, dim)
		.await
}

async fn wait_for_population(
	services: &Services,
	mxc: &Mxc<'_>,
	timeout_ms: Duration,
) -> Result<FileMeta> {
	async fn inner(services: &Services, mxc: &Mxc<'_>) -> Result<FileMeta> {
		if !services.media.exists(mxc).await {
			return Err!(Request(NotFound("Media not found.")));
		}
		loop {
			if let Ok(Some(filemeta)) = services.media.get(mxc).await {
				return Ok(filemeta);
			}
			// TODO(async-media): A notify/send mechanism would be better than polling.
			tokio::time::sleep(Duration::from_millis(100)).await;
		}
	}
	tokio::time::timeout(
		timeout_ms
			.checked_sub(Duration::from_millis(500))
			.unwrap_or(Duration::from_millis(0)),
		inner(services, mxc),
	)
	.await
	.map_err(|_| err!(Request(NotYetUploaded("Media was not ready in time."))))?
}
async fn fetch_file_meta(
	services: &Services,
	mxc: &Mxc<'_>,
	user: &UserId,
	timeout_ms: Duration,
) -> Result<FileMeta> {
	if services.globals.server_is_ours(mxc.server_name) {
		let result = wait_for_population(services, mxc, timeout_ms).await;
		if let Ok(filemeta) = result {
			return Ok(filemeta);
		}
		return Err!(Request(NotFound("Local media not found.")));
	}

	if let Some(filemeta) = services.media.get(mxc).await? {
		return Ok(filemeta);
	}

	services
		.media
		.fetch_remote_content(mxc, Some(user), None, timeout_ms)
		.await
}
