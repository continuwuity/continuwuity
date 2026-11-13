use std::{fmt::Debug, time::Duration};

use conduwuit::{
	Err, Result, debug_warn, err,
	utils::{content_disposition::make_content_disposition, response::LimitReadExt},
};
use http::header::{CONTENT_DISPOSITION, CONTENT_TYPE, HeaderValue};
use ruma::{
	ServerName, UserId,
	api::{
		OutgoingRequest,
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

impl super::Service {
	pub async fn fetch_remote_thumbnail(
		&self,
		mxc: &Mxc<'_>,
		user: Option<&UserId>,
		server: Option<&ServerName>,
		timeout_ms: Duration,
		dim: &Dim,
	) -> Result<FileMeta> {
		self.check_fetch_authorized(mxc)?;

		self.fetch_thumbnail_authenticated(mxc, user, server, timeout_ms, dim)
			.await
	}

	pub async fn fetch_remote_content(
		&self,
		mxc: &Mxc<'_>,
		user: Option<&UserId>,
		server: Option<&ServerName>,
		timeout_ms: Duration,
	) -> Result<FileMeta> {
		self.check_fetch_authorized(mxc)?;

		self.fetch_content_authenticated(mxc, user, server, timeout_ms)
			.await
			.inspect_err(|error| {
				debug_warn!(
					%mxc,
					?user,
					?server,
					?error,
					"Authenticated fetch of remote content failed"
				);
			})
	}

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
			| FileOrLocation::Location(location) =>
				self.handle_location(mxc, user, &location).await,
			| _ => Err!("Unknown content in response"),
		}
	}

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
			| FileOrLocation::Location(location) =>
				self.handle_location(mxc, user, &location).await,
			| _ => Err!("Unknown content in response"),
		}
	}

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
			content_type: content.content_type,
			content_disposition: Some(content_disposition),
		})
	}

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
			content_type: content.content_type,
			content_disposition: Some(content_disposition),
		})
	}

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

	async fn location_request(&self, location: &str) -> Result<FileMeta> {
		let response = self
			.services
			.client
			.external_resource
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

	fn check_fetch_authorized(&self, mxc: &Mxc<'_>) -> Result<()> {
		if self
			.services
			.moderation
			.is_remote_server_media_downloads_forbidden(mxc.server_name)
		{
			// we'll lie to the client and say the blocked server's media was
			// not found and log. the client has no way of telling anyways so
			// this is a security bonus.
			debug_warn!(%mxc, "Received request for media on blocklisted server");
			return Err!(Request(NotFound("Media not found.")));
		}

		Ok(())
	}
}
