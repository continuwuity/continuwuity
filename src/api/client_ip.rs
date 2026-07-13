use core::net::{IpAddr, SocketAddr};
use std::ops::Deref;

use axum::{
	extract::{ConnectInfo, FromRequestParts},
	response::{IntoResponse, Response},
};
use conduwuit::{debug_info, debug_warn};
use http::{HeaderMap, StatusCode, request::Parts};
use service::Services;

#[derive(Debug, PartialEq)]
pub enum ClientIpError {
	Header(client_ip::Error),
	Direct,
}

impl IntoResponse for ClientIpError {
	fn into_response(self) -> Response {
		let text = match self {
			| Self::Header(e) => format!("{e}"),
			| Self::Direct => "Failed to extract IP from ConnectionInfo".to_owned(),
		};
		(StatusCode::INTERNAL_SERVER_ERROR, text).into_response()
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientIp(pub IpAddr);

type PartsToIpFn = Box<dyn Fn(&Parts) -> Result<ClientIp, ClientIpError>>;

impl ClientIp {
	fn from_header<T>(func: T) -> PartsToIpFn
	where
		T: Fn(&HeaderMap) -> Result<IpAddr, client_ip::Error> + 'static,
	{
		Box::new(move |parts: &Parts| {
			func(&parts.headers)
				.map(Self)
				.map_err(ClientIpError::Header)
		})
	}

	fn from_connection_info(parts: &Parts) -> Result<Self, ClientIpError> {
		parts
			.extensions
			.get::<ConnectInfo<SocketAddr>>()
			.ok_or_else(|| ClientIpError::Direct)
			.map(|ConnectInfo(addr)| Self(addr.ip()))
	}

	fn for_source(source: &str) -> Option<PartsToIpFn> {
		match source {
			| "cf_connecting_ip" => Some(Self::from_header(client_ip::cf_connecting_ip)),
			| "cloudfront_viewer_address" =>
				Some(Self::from_header(client_ip::cloudfront_viewer_address)),
			| "fly_client_ip" => Some(Self::from_header(client_ip::fly_client_ip)),
			| "x_forwarded_for" => Some(Self::from_header(client_ip::rightmost_x_forwarded_for)),
			| "true_client_ip" => Some(Self::from_header(client_ip::true_client_ip)),
			| "x_envoy_external_address" =>
				Some(Self::from_header(client_ip::x_envoy_external_address)),
			| "x_real_ip" => Some(Self::from_header(client_ip::x_real_ip)),
			| "direct" => Some(Box::new(Self::from_connection_info)),
			| s => {
				debug_warn!("Invalid client IP source option supplied, skipping: {s}");
				None
			},
		}
	}

	fn for_config(
		parts: &Parts,
		accepted_ip_sources: &[String],
		request_ip_source: Option<&String>,
	) -> Result<Self, ClientIpError> {
		accepted_ip_sources
			.iter()
			.filter_map(|s| Self::for_source(s))
			.map(|f| f(parts))
			.reduce(Result::or)
			.or_else(|| {
				debug_info!(
					"No (valid) options set in `accepted_ip_sources`; falling back to \
					 `request_ip_source`"
				);
				request_ip_source
					.and_then(|s| Self::for_source(s))
					.map(|f| f(parts).or_else(|_| Self::from_connection_info(parts)))
			})
			.unwrap_or_else(|| {
				debug_info!(
					"No (valid) options set in `accepted_ip_sources` or `request_ip_source`; \
					 using peer address"
				);
				Self::from_connection_info(parts)
			})
	}
}

impl<S> FromRequestParts<S> for ClientIp
where
	S: Deref<Target = Services> + Sync,
{
	type Rejection = ClientIpError;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		Self::for_config(
			parts,
			&state.config.accepted_ip_sources,
			state.config.request_ip_source.as_ref(),
		)
	}
}

#[cfg(test)]
mod tests {
	use std::net::{Ipv4Addr, Ipv6Addr};

	use super::*;

	const SOCKET_V4: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1));
	const HEADER_V4: IpAddr = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1));

	const SOCKET_V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 1));
	const HEADER_V6: IpAddr = IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 2));

	fn parts(peer: Option<IpAddr>, headers: &[(&str, &str)]) -> Parts {
		let mut builder = http::Request::builder();
		for (name, value) in headers {
			builder = builder.header(*name, *value);
		}

		let (mut parts, ()) = builder.body(()).unwrap().into_parts();
		if let Some(peer) = peer {
			parts
				.extensions
				.insert(ConnectInfo(SocketAddr::new(peer, 8448)));
		}

		parts
	}

	fn extract(
		parts: &Parts,
		accepted_ip_sources: &[String],
		request_ip_source: Option<&String>,
	) -> Result<IpAddr, ClientIpError> {
		ClientIp::for_config(parts, accepted_ip_sources, request_ip_source).map(|ClientIp(ip)| ip)
	}

	#[tokio::test]
	async fn resolves_from_configured_source_ipv4() {
		let parts = parts(Some(SOCKET_V4), &[("x-real-ip", &HEADER_V4.to_string())]);

		assert_eq!(extract(&parts, &["x_real_ip".to_owned()], None), Ok(HEADER_V4));
		assert_eq!(extract(&parts, &[], Some(&"x_real_ip".to_owned())), Ok(HEADER_V4));
	}

	#[tokio::test]
	async fn resolves_from_configured_source_ipv6() {
		let parts = parts(Some(SOCKET_V6), &[("x-real-ip", &HEADER_V6.to_string())]);

		assert_eq!(extract(&parts, &["x_real_ip".to_owned()], None), Ok(HEADER_V6));
		assert_eq!(extract(&parts, &[], Some(&"x_real_ip".to_owned())), Ok(HEADER_V6));
	}

	#[tokio::test]
	async fn accepted_ip_sources_no_fall_back_to_peer_when_header_missing() {
		let parts = parts(Some(SOCKET_V4), &[]);

		extract(&parts, &["x_real_ip".to_owned()], None).unwrap_err();
	}

	#[tokio::test]
	async fn request_ip_source_falls_back_to_peer_when_header_missing() {
		let parts = parts(Some(SOCKET_V4), &[]);

		assert_eq!(extract(&parts, &[], Some(&"x_real_ip".to_owned())), Ok(SOCKET_V4));
	}

	#[tokio::test]
	async fn accepted_ip_sources_no_fall_back_to_peer_when_header_unparsable() {
		let parts = parts(Some(SOCKET_V4), &[("x-real-ip", "not-an-ip")]);

		extract(&parts, &["x_real_ip".to_owned()], None).unwrap_err();
	}

	#[tokio::test]
	async fn request_ip_source_falls_back_to_peer_when_header_unparsable() {
		let parts = parts(Some(SOCKET_V4), &[("x-real-ip", "not-an-ip")]);

		assert_eq!(extract(&parts, &[], Some(&"x_real_ip".to_owned())), Ok(SOCKET_V4));
	}

	#[tokio::test]
	async fn accepted_ip_sources_falls_back_when_configured() {
		let parts = parts(Some(SOCKET_V4), &[]);

		assert_eq!(
			extract(&parts, &["x_real_ip".to_owned(), "direct".to_owned()], None),
			Ok(SOCKET_V4)
		);
	}

	#[tokio::test]
	async fn falls_back_to_peer_no_configuration() {
		let parts = parts(Some(SOCKET_V4), &[("x-real-ip", &HEADER_V4.to_string())]);

		assert_eq!(extract(&parts, &[], None), Ok(SOCKET_V4));
	}
}
