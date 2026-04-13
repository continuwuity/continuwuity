use std::{borrow::Cow, fmt::Debug, mem};

use bytes::Bytes;
use conduwuit::{
	Err, Error, Result, debug, debug_error, debug_warn, err, implement, trace,
	utils::response::LimitReadExt,
};
use ipaddress::IPAddress;
use reqwest::{Client, Method, Request, Response, Url};
use ruma::{
	ServerName,
	api::{
		EndpointError, IncomingResponse, OutgoingRequest, SupportedVersions,
		auth_scheme::{AuthScheme, NoAuthentication},
		error::Error as RumaError,
		federation::authentication::{ServerSignatures, ServerSignaturesInput},
		path_builder::PathBuilder,
	},
};

use crate::{SUPPORTED_VERSIONS, resolver::actual::ActualDest};

/// Sends a request to a federation server
#[implement(super::Service)]
#[tracing::instrument(skip_all, name = "request", level = "debug")]
pub async fn execute<'i, T>(&self, dest: &ServerName, request: T) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest<
			Authentication = ServerSignatures,
			PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
		> + Debug
		+ Send,
{
	let client = &self.services.client.federation;
	self.execute_signed(client, dest, request).await
}

/// Like execute() but with a very large timeout
#[implement(super::Service)]
#[tracing::instrument(skip_all, name = "synapse", level = "debug")]
pub async fn execute_synapse<'i, T>(
	&self,
	dest: &ServerName,
	request: T,
) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest<
			Authentication = ServerSignatures,
			PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
		> + Debug
		+ Send,
{
	let client = &self.services.client.synapse;
	self.execute_signed(client, dest, request).await
}

#[implement(super::Service)]
pub async fn execute_unauthenticated<'i, T>(
	&self,
	dest: &ServerName,
	request: T,
) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest<
			Authentication = NoAuthentication,
			PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
		> + Debug
		+ Send,
{
	let client = &self.services.client.federation;

	self.execute_on(client, dest, request, ()).await
}

#[implement(super::Service)]
pub async fn execute_signed<'i, T>(
	&self,
	client: &Client,
	dest: &ServerName,
	request: T,
) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest<
			Authentication = ServerSignatures,
			PathBuilder: PathBuilder<Input<'i>: FederationPathBuilderInput>,
		> + Send,
{
	let authentication = ServerSignaturesInput::new(
		self.services.server.name.clone(),
		dest.to_owned(),
		self.services.server_keys.keypair(),
	);

	self.execute_on(client, dest, request, authentication).await
}

#[implement(super::Service)]
#[tracing::instrument(name = "fed", level = "info", skip(self, client, request, authentication))]
pub async fn execute_on<'i, T, PathBuilderInput>(
	&self,
	client: &Client,
	dest: &ServerName,
	request: T,
	authentication: <T::Authentication as AuthScheme>::Input<'_>,
) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest<PathBuilder: PathBuilder<Input<'i> = PathBuilderInput>> + Send,
	PathBuilderInput: FederationPathBuilderInput,
{
	if !self.services.server.config.allow_federation {
		return Err!(Config("allow_federation", "Federation is disabled."));
	}

	if self.services.moderation.is_remote_server_forbidden(dest) {
		return Err!(Request(Forbidden(debug_warn!("Federation with {dest} is not allowed."))));
	}

	let actual = self.services.resolver.get_actual_dest(dest).await?;

	let request = Request::try_from(request.try_into_http_request::<Vec<u8>>(
		actual.string().as_str(),
		authentication,
		PathBuilderInput::create(),
	)?)?;
	self.validate_url(request.url())?;
	self.services.server.check_running()?;

	self.perform::<T>(dest, &actual, request, client).await
}

#[implement(super::Service)]
async fn perform<T>(
	&self,
	dest: &ServerName,
	actual: &ActualDest,
	request: Request,
	client: &Client,
) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest + Send,
{
	let url = request.url().clone();
	let method = request.method().clone();

	debug!(%method, %url, "Sending request");
	match client.execute(request).await {
		| Ok(response) =>
			self.handle_response::<T>(dest, actual, &method, &url, response)
				.await,
		| Err(error) =>
			Err(handle_error(actual, &method, &url, error).expect_err("always returns error")),
	}
}

#[implement(super::Service)]
fn validate_url(&self, url: &Url) -> Result<()> {
	if let Some(url_host) = url.host_str() {
		if let Ok(ip) = IPAddress::parse(url_host) {
			trace!("Checking request URL IP {ip:?}");
			self.services.resolver.validate_ip(&ip)?;
		}
	}

	Ok(())
}

#[implement(super::Service)]
async fn handle_response<T>(
	&self,
	dest: &ServerName,
	actual: &ActualDest,
	method: &Method,
	url: &Url,
	response: Response,
) -> Result<T::IncomingResponse>
where
	T: OutgoingRequest + Send,
{
	const HUGE_ENDPOINTS: [&str; 2] =
		["/_matrix/federation/v2/send_join/", "/_matrix/federation/v2/state/"];
	let size_limit: u64 = if HUGE_ENDPOINTS.iter().any(|e| url.path().starts_with(e)) {
		// Some federation endpoints can return huge response bodies, so we'll bump the
		// limit for those endpoints specifically.
		self.services
			.server
			.config
			.max_request_size
			.saturating_mul(10)
	} else {
		self.services.server.config.max_request_size
	}
	.try_into()
	.expect("size_limit (usize) should fit within a u64");
	let response = into_http_response(dest, actual, method, url, response, size_limit).await?;

	T::IncomingResponse::try_from_http_response(response)
		.map_err(|e| err!(BadServerResponse("Server returned bad 200 response: {e:?}")))
}

async fn into_http_response(
	dest: &ServerName,
	actual: &ActualDest,
	method: &Method,
	url: &Url,
	mut response: Response,
	max_size: u64,
) -> Result<http::Response<Bytes>> {
	let status = response.status();
	trace!(
		%status, %method,
		request_url = %url,
		response_url = %response.url(),
		"Received response from {}",
		actual.string(),
	);

	let mut http_response_builder = http::Response::builder()
		.status(status)
		.version(response.version());

	mem::swap(
		response.headers_mut(),
		http_response_builder
			.headers_mut()
			.expect("http::response::Builder is usable"),
	);

	trace!("Waiting for response body...");
	let http_response = http_response_builder
		.body(
			response
				.limit_read(max_size)
				.await
				.unwrap_or_default()
				.into(),
		)
		.expect("reqwest body is valid http body");

	debug!("Got {status:?} for {method} {url}");
	if !status.is_success() {
		return Err(Error::Federation(
			dest.to_owned(),
			RumaError::from_http_response(http_response),
		));
	}

	Ok(http_response)
}

fn handle_error(
	actual: &ActualDest,
	method: &Method,
	url: &Url,
	mut e: reqwest::Error,
) -> Result {
	if e.is_timeout() || e.is_connect() {
		e = e.without_url();
		debug_warn!("{e:?}");
	} else if e.is_redirect() {
		debug_error!(
			%method,
			%url,
			final_url = e.url().map(tracing::field::display),
			"Redirect loop {}: {}",
			actual.host,
			e,
		);
	} else {
		debug_error!("{e:?}");
	}

	Err(e.into())
}

/// A trait for the input types of acceptable path builders for outgoing
/// federation requests.
///
/// Ruma uses Rust's type system to encode the versioning scheme of endpoints in
/// the Matrix spec. Every endpoint has a `PathBuilder` associated type, which
/// has an `Input` associated type. Endpoints with multiple versions have
/// `VersionHistory` as their `PathBuilder`, which has `SupportedVersions`
/// as its `Input` type. Endpoints with no version have `SinglePath` as their
/// `PathBuilder`, which has `()` as its `Input` type. Both `SupportedVersions`
/// and `()` can be created out of thin air using static data (or no data at
/// all). This property is what the `FederationPathBuilderInput` trait
/// represents.
///
/// This trait allows the federation sender service's functions to accept
/// requests for either versioned or unversioned endpoints, by requiring that
/// the `Input` of the `PathBuilder` of the endpoint implements
/// `FederationPathBuilderInput`.
pub trait FederationPathBuilderInput {
	fn create() -> Self;
}

impl FederationPathBuilderInput for () {
	fn create() -> Self {}
}

impl FederationPathBuilderInput for Cow<'_, SupportedVersions> {
	fn create() -> Self { Cow::Borrowed(&SUPPORTED_VERSIONS) }
}
