use std::time::Duration;

use axum::extract::State;
use axum_client_ip::InsecureClientIp;
use conduwuit::{
	Err, Result,
	debug::DebugInspect,
	debug_info, err,
	utils::{self, content_disposition::make_content_disposition, math::ruma_from_usize},
	warn,
};
use conduwuit_service::{
	Services,
	media::{CACHE_CONTROL_IMMUTABLE, CORP_CROSS_ORIGIN, Dim, FileMeta, MXC_LENGTH},
};
use reqwest::Url;
use ruma::{
	Mxc, OwnedServerName, UserId,
	api::client::{
		authenticated_media,
		authenticated_media::{
			get_content, get_content_as_filename, get_content_thumbnail, get_media_config,
			get_media_preview,
		},
		media::create_content,
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

	if let Err(e) = services
		.media
		.create(mxc, Some(user), Some(&content_disposition), content_type, &body.file)
		.await
	{
		err!("Failed to save uploaded media: {e}");
		return Err!(Request(Unknown("Failed to save uploaded media")));
	}

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

async fn dispatch_redaction(
	server_name: OwnedServerName,
	media_id: String,
	servers: Vec<OwnedServerName>,
	services: crate::State,
) {
	for server in servers {
		if services.globals.server_is_ours(&server) {
			continue;
		}

		debug_info!("Asking {server} to redact media mxc://{server_name}/{media_id}");
		let _ = services
			.federation
			.execute(&server, authenticated_media::redact::unstable::Request {
				server_name: server_name.clone(),
				media_id: media_id.clone(),
			})
			.await
			.debug_inspect(|_| {
				debug_info!("Asked {server} to redact media mxc://{server_name}/{media_id}");
			})
			.inspect_err(|e| {
				warn!(
					"Failed to ask {server} to redact media mxc://{server_name}/{media_id}: {e}"
				);
			})
			.ok();
	}
}

#[tracing::instrument(
	name = "media_redact",
	level = "debug",
	skip_all,
	fields(%_client),
)]
pub(crate) async fn redact_media_route(
	State(services): State<crate::State>,
	InsecureClientIp(_client): InsecureClientIp,
	body: Ruma<authenticated_media::redact::unstable::Request>,
) -> Result<authenticated_media::redact::unstable::Response> {
	let user = body.sender_user();

	let mxc = Mxc {
		server_name: &body.server_name,
		media_id: &body.media_id,
	};

	if !services.media.user_owns(user, &mxc).await {
		return Err!(Request(Forbidden("You do not have permission to redact this attachment.")));
	}

	services.media.redact(&mxc).await?;

	// TODO: This should be a persistent background task
	let servers = services.media.get_interested_servers(&mxc).await;
	tokio::spawn(dispatch_redaction(
		mxc.server_name.to_owned(),
		mxc.media_id.to_owned(),
		servers,
		services,
	));

	Ok(authenticated_media::redact::unstable::Response {})
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

async fn fetch_file_meta(
	services: &Services,
	mxc: &Mxc<'_>,
	user: &UserId,
	timeout_ms: Duration,
) -> Result<FileMeta> {
	if let Some(filemeta) = services.media.get(mxc).await? {
		return Ok(filemeta);
	}

	if services.globals.server_is_ours(mxc.server_name) {
		return Err!(Request(NotFound("Local media not found.")));
	}

	services
		.media
		.fetch_remote_content(mxc, Some(user), None, timeout_ms)
		.await
}
