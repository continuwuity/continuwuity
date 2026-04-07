use std::{fmt::Debug, time::Duration};

use conduwuit::{
	Err, Error, Result, debug_warn, err, implement,
	utils::{content_disposition::make_content_disposition, response::LimitReadExt},
};
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderValue};
use ruma::{
	ServerName, UserId,
	api::{
		OutgoingRequest,
		auth_scheme::NoAccessToken,
		client::media,
		error::ErrorKind::{NotFound, Unrecognized},
		federation::{
			self,
			authenticated_media::{Content, FileOrLocation},
			authentication::ServerSignatures,
		},
		path_builder::PathBuilder,
	},
};

use super::{Dim, FileMeta};
use crate::{federation::FederationPathBuilderInput, media::mxc::Mxc};

#[implement(super::Service)]
pub async fn fetch_remote_thumbnail(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	server: Option<&ServerName>,
	timeout_ms: Duration,
	dim: &Dim,
) -> Result<FileMeta> {
	self.check_fetch_authorized(mxc)?;

	let result = self
		.fetch_thumbnail_authenticated(mxc, user, server, timeout_ms, dim)
		.await;

	if let Err(Error::Request(NotFound, ..)) = &result {
		return self
			.fetch_thumbnail_unauthenticated(mxc, user, server, timeout_ms, dim)
			.await;
	}

	result
}

#[implement(super::Service)]
pub async fn fetch_remote_content(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	server: Option<&ServerName>,
	timeout_ms: Duration,
) -> Result<FileMeta> {
	self.check_fetch_authorized(mxc)?;

	let result = self
		.fetch_content_authenticated(mxc, user, server, timeout_ms)
		.await
		.inspect_err(|error| {
			debug_warn!(
				%mxc,
				?user,
				?server,
				?error,
				"Authenticated fetch of remote content failed"
			);
		});

	if let Err(Error::Request(Unrecognized, ..)) = &result {
		return self
			.fetch_content_unauthenticated(mxc, user, server, timeout_ms)
			.await;
	}

	result
}

#[implement(super::Service)]
async fn fetch_thumbnail_authenticated(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	server: Option<&ServerName>,
	timeout_ms: Duration,
	dim: &Dim,
) -> Result<FileMeta> {
	use federation::authenticated_media::get_content_thumbnail::v1::{Request, Response};

	let mut request = Request::new(mxc.media_id.into(), dim.width.into(), dim.height.into());
	request.method = Some(dim.method.clone());
	request.animated = Some(true);
	request.timeout_ms = timeout_ms;

	let Response { content, .. } = self.federation_request(mxc, server, request).await?;

	match content {
		| FileOrLocation::File(content) =>
			self.handle_thumbnail_file(mxc, user, dim, content).await,
		| FileOrLocation::Location(location) => self.handle_location(mxc, user, &location).await,
		| _ => Err!("Unknown content in response"),
	}
}

#[implement(super::Service)]
async fn fetch_content_authenticated(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	server: Option<&ServerName>,
	timeout_ms: Duration,
) -> Result<FileMeta> {
	use federation::authenticated_media::get_content::v1::{Request, Response};

	let mut request = Request::new(mxc.media_id.into());
	request.timeout_ms = timeout_ms;

	let Response { content, .. } = self.federation_request(mxc, server, request).await?;

	match content {
		| FileOrLocation::File(content) => self.handle_content_file(mxc, user, content).await,
		| FileOrLocation::Location(location) => self.handle_location(mxc, user, &location).await,
		| _ => Err!("Unknown content in response"),
	}
}

#[allow(deprecated)]
#[implement(super::Service)]
async fn fetch_thumbnail_unauthenticated(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	server: Option<&ServerName>,
	timeout_ms: Duration,
	dim: &Dim,
) -> Result<FileMeta> {
	use media::get_content_thumbnail::v3::{Request, Response};

	let mut request = Request::new(
		mxc.media_id.into(),
		mxc.server_name.into(),
		dim.width.into(),
		dim.height.into(),
	);
	request.allow_redirect = true;
	request.allow_remote = true;
	request.animated = Some(true);
	request.method = Some(dim.method.clone());
	request.timeout_ms = timeout_ms;

	let Response {
		file, content_type, content_disposition, ..
	} = self
		.federation_request_legacy_media(mxc, server, request)
		.await?;

	let content = Content::new(file, content_type.unwrap(), content_disposition.unwrap());

	self.handle_thumbnail_file(mxc, user, dim, content).await
}

#[allow(deprecated)]
#[implement(super::Service)]
async fn fetch_content_unauthenticated(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	server: Option<&ServerName>,
	timeout_ms: Duration,
) -> Result<FileMeta> {
	use media::get_content::v3::{Request, Response};

	let mut request = Request::new(mxc.media_id.into(), mxc.server_name.into());
	request.allow_remote = true;
	request.allow_redirect = true;
	request.timeout_ms = timeout_ms;

	let Response {
		file, content_type, content_disposition, ..
	} = self
		.federation_request_legacy_media(mxc, server, request)
		.await?;

	let content = Content::new(file, content_type.unwrap(), content_disposition.unwrap());

	self.handle_content_file(mxc, user, content).await
}

#[implement(super::Service)]
async fn handle_thumbnail_file(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	dim: &Dim,
	content: Content,
) -> Result<FileMeta> {
	let content_disposition = make_content_disposition(
		content.content_disposition.as_ref(),
		content.content_type.as_deref(),
		None,
	);

	self.upload_thumbnail(
		mxc,
		user,
		Some(&content_disposition),
		content.content_type.as_deref(),
		dim,
		&content.file,
	)
	.await
	.map(|()| FileMeta {
		content: Some(content.file),
		content_type: content.content_type.map(Into::into),
		content_disposition: Some(content_disposition),
	})
}

#[implement(super::Service)]
async fn handle_content_file(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	content: Content,
) -> Result<FileMeta> {
	let content_disposition = make_content_disposition(
		content.content_disposition.as_ref(),
		content.content_type.as_deref(),
		None,
	);

	self.create(
		mxc,
		user,
		Some(&content_disposition),
		content.content_type.as_deref(),
		&content.file,
	)
	.await
	.map(|()| FileMeta {
		content: Some(content.file),
		content_type: content.content_type.map(Into::into),
		content_disposition: Some(content_disposition),
	})
}

#[implement(super::Service)]
async fn handle_location(
	&self,
	mxc: &Mxc<'_>,
	user: Option<&UserId>,
	location: &str,
) -> Result<FileMeta> {
	self.location_request(location).await.map_err(|error| {
		err!(Request(NotFound(
			debug_warn!(%mxc, user = user.map(tracing::field::display), ?location, ?error, "Fetching media from location failed")
		)))
	})
}

#[implement(super::Service)]
async fn location_request(&self, location: &str) -> Result<FileMeta> {
	let response = self
		.services
		.client
		.extern_media
		.get(location)
		.send()
		.await?;

	let content_type = response
		.headers()
		.get(CONTENT_TYPE)
		.map(HeaderValue::to_str)
		.and_then(Result::ok)
		.map(str::to_owned);

	let content_disposition = response
		.headers()
		.get(CONTENT_DISPOSITION)
		.map(HeaderValue::as_bytes)
		.map(TryFrom::try_from)
		.and_then(Result::ok);

	response
		.limit_read(
			self.services
				.server
				.config
				.max_request_size
				.try_into()
				.expect("u64 should fit in usize"),
		)
		.await
		.map(|content| FileMeta {
			content: Some(content),
			content_type: content_type.clone(),
			content_disposition: Some(make_content_disposition(
				content_disposition.as_ref(),
				content_type.as_deref(),
				None,
			)),
		})
}

#[implement(super::Service)]
async fn federation_request<'i, Request>(
	&self,
	mxc: &Mxc<'_>,
	server: Option<&ServerName>,
	request: Request,
) -> Result<Request::IncomingResponse>
where
	Request: OutgoingRequest<
			Authentication = ServerSignatures,
			PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
		> + Debug
		+ Send,
{
	self.services
		.sending
		.send_federation_request(server.unwrap_or(mxc.server_name), request)
		.await
}

#[implement(super::Service)]
async fn federation_request_legacy_media<'i, Request>(
	&self,
	mxc: &Mxc<'_>,
	server: Option<&ServerName>,
	request: Request,
) -> Result<Request::IncomingResponse>
where
	Request: OutgoingRequest<
			Authentication = NoAccessToken,
			PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
		> + Debug
		+ Send,
{
	self.services
		.sending
		.send_legacy_media_request(server.unwrap_or(mxc.server_name), request)
		.await
}

#[implement(super::Service)]
#[allow(deprecated)]
pub async fn fetch_remote_thumbnail_legacy(
	&self,
	body: &media::get_content_thumbnail::v3::Request,
) -> Result<media::get_content_thumbnail::v3::Response> {
	let mxc = Mxc {
		server_name: &body.server_name,
		media_id: &body.media_id,
	};

	let mut request = media::get_content_thumbnail::v3::Request::new(
		body.media_id.clone(),
		body.server_name.clone(),
		body.width,
		body.height,
	);
	request.method = body.method.clone();
	request.allow_remote = body.allow_remote;
	request.allow_redirect = body.allow_redirect;
	request.animated = body.animated;
	request.timeout_ms = body.timeout_ms;

	self.check_legacy_freeze()?;
	self.check_fetch_authorized(&mxc)?;
	let response = self
		.services
		.sending
		.send_legacy_media_request(mxc.server_name, request)
		.await?;

	let dim = Dim::from_ruma(body.width, body.height, body.method.clone())?;
	self.upload_thumbnail(
		&mxc,
		None,
		None,
		response.content_type.as_deref(),
		&dim,
		&response.file,
	)
	.await?;

	Ok(response)
}

#[implement(super::Service)]
#[allow(deprecated)]
pub async fn fetch_remote_content_legacy(
	&self,
	mxc: &Mxc<'_>,
	allow_redirect: bool,
	timeout_ms: Duration,
) -> Result<media::get_content::v3::Response, Error> {
	let mut request =
		media::get_content::v3::Request::new(mxc.media_id.into(), mxc.server_name.into());
	request.allow_remote = true;
	request.allow_redirect = allow_redirect;
	request.timeout_ms = timeout_ms;

	self.check_legacy_freeze()?;
	self.check_fetch_authorized(mxc)?;
	let response = self
		.services
		.sending
		.send_legacy_media_request(mxc.server_name, request)
		.await?;

	let content_disposition = make_content_disposition(
		response.content_disposition.as_ref(),
		response.content_type.as_deref(),
		None,
	);

	self.create(
		mxc,
		None,
		Some(&content_disposition),
		response.content_type.as_deref(),
		&response.file,
	)
	.await?;

	Ok(response)
}

#[implement(super::Service)]
fn check_fetch_authorized(&self, mxc: &Mxc<'_>) -> Result<()> {
	if self
		.services
		.moderation
		.is_remote_server_media_downloads_forbidden(mxc.server_name)
	{
		// we'll lie to the client and say the blocked server's media was not found and
		// log. the client has no way of telling anyways so this is a security bonus.
		debug_warn!(%mxc, "Received request for media on blocklisted server");
		return Err!(Request(NotFound("Media not found.")));
	}

	Ok(())
}

#[implement(super::Service)]
fn check_legacy_freeze(&self) -> Result<()> {
	self.services
		.server
		.config
		.freeze_legacy_media
		.then_some(())
		.ok_or(err!(Request(NotFound("Remote media is frozen."))))
}
