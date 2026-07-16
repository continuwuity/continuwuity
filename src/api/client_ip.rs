use std::{
	convert::Infallible,
	net::{IpAddr, Ipv4Addr, SocketAddr},
};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum_client_ip::ClientIp as SourcedClientIp;
use conduwuit::debug_warn;
use http::request::Parts;

const UNKNOWN_IP: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// [`ClientIp`] extractor that falls back to the connection peer address
/// instead of rejecting the request when `request_ip_source` can't be resolved.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClientIp(pub IpAddr);

impl<S> FromRequestParts<S> for ClientIp
where
	S: Sync,
{
	type Rejection = Infallible;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
		let ip = match SourcedClientIp::from_request_parts(parts, state).await {
			| Ok(SourcedClientIp(ip)) => ip,
			| Err(rejection) => {
				debug_warn!(
					%rejection,
					"Could not resolve client IP from request_ip_source; using peer address"
				);
				parts
					.extensions
					.get::<ConnectInfo<SocketAddr>>()
					.map_or(UNKNOWN_IP, |ConnectInfo(addr)| addr.ip())
			},
		};

		Ok(Self(ip))
	}
}

#[cfg(test)]
mod tests {
	use std::net::Ipv6Addr;

	use axum_client_ip::ClientIpSource;

	use super::*;

	const PEER: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 7)), 8448);
	const PEER_V6: SocketAddr =
		SocketAddr::new(IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 1)), 8448);

	fn parts(
		source: Option<ClientIpSource>,
		peer: Option<SocketAddr>,
		headers: &[(&str, &str)],
	) -> Parts {
		let mut builder = http::Request::builder();
		for (name, value) in headers {
			builder = builder.header(*name, *value);
		}

		let (mut parts, ()) = builder.body(()).unwrap().into_parts();
		if let Some(source) = source {
			parts.extensions.insert(source);
		}
		if let Some(peer) = peer {
			parts.extensions.insert(ConnectInfo(peer));
		}

		parts
	}

	async fn extract(mut parts: Parts) -> IpAddr {
		let ClientIp(ip) = ClientIp::from_request_parts(&mut parts, &()).await.unwrap();
		ip
	}

	#[tokio::test]
	async fn resolves_from_configured_source() {
		let parts =
			parts(Some(ClientIpSource::XRealIp), Some(PEER), &[("x-real-ip", "203.0.113.5")]);

		assert_eq!(extract(parts).await, IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)));
	}

	#[tokio::test]
	async fn resolves_ipv6_from_configured_source() {
		let parts =
			parts(Some(ClientIpSource::XRealIp), Some(PEER), &[("x-real-ip", "2001:db8::2")]);

		assert_eq!(
			extract(parts).await,
			IpAddr::V6(Ipv6Addr::new(0x2001, 0xDB8, 0, 0, 0, 0, 0, 2))
		);
	}

	#[tokio::test]
	async fn falls_back_to_peer_when_header_missing() {
		let parts = parts(Some(ClientIpSource::XRealIp), Some(PEER), &[]);

		assert_eq!(extract(parts).await, PEER.ip());
	}

	#[tokio::test]
	async fn falls_back_to_ipv6_peer_when_header_missing() {
		let parts = parts(Some(ClientIpSource::XRealIp), Some(PEER_V6), &[]);

		assert_eq!(extract(parts).await, PEER_V6.ip());
	}

	#[tokio::test]
	async fn falls_back_to_peer_when_header_unparsable() {
		let parts =
			parts(Some(ClientIpSource::XRealIp), Some(PEER), &[("x-real-ip", "not-an-ip")]);

		assert_eq!(extract(parts).await, PEER.ip());
	}

	#[tokio::test]
	async fn falls_back_to_peer_when_source_unset() {
		let parts = parts(None, Some(PEER), &[("x-real-ip", "203.0.113.5")]);

		assert_eq!(extract(parts).await, PEER.ip());
	}

	#[tokio::test]
	async fn falls_back_to_unspecified_without_peer() {
		let parts = parts(Some(ClientIpSource::XRealIp), None, &[]);

		assert_eq!(extract(parts).await, UNKNOWN_IP);
	}
}
