use std::{
	net::{IpAddr, SocketAddr},
	sync::Arc,
};

use hickory_resolver::{Resolver, net::runtime::TokioRuntimeProvider};
use reqwest::dns::{Addrs, Resolving};

/// A simple struct to implement reqwest::dns::Resolve, as that is what's
/// required by the reqwest client builder. Most code is based on
/// reqwest::dns::HickoryDnsResolver, which we here have to almost re-create
/// because it's pub(crate) instead of pub:
pub struct DnsResolver {
	resolver: Arc<TokioResolver>,
}

type TokioResolver = Resolver<TokioRuntimeProvider>;

impl DnsResolver {
	pub(crate) fn new(resolver: Arc<TokioResolver>) -> Self { Self { resolver } }
}

struct SocketAddrs {
	iter: std::vec::IntoIter<IpAddr>,
}

impl Iterator for SocketAddrs {
	type Item = SocketAddr;

	fn next(&mut self) -> Option<Self::Item> {
		self.iter.next().map(|ip_addr| SocketAddr::new(ip_addr, 0))
	}
}

impl reqwest::dns::Resolve for DnsResolver {
	fn resolve(&self, name: reqwest::dns::Name) -> Resolving {
		let resolver = self.resolver.clone();
		Box::pin(async move {
			let lookup = resolver.lookup_ip(name.as_str()).await?;
			let addrs: Addrs = Box::new(SocketAddrs {
				iter: lookup.iter().collect::<Vec<_>>().into_iter(),
			});
			Ok(addrs)
		})
	}
}

// openidconnect is weird and vendors reqwest (?), therefore we have to
// completely re-implement Resolve with different types
impl openidconnect::reqwest::dns::Resolve for DnsResolver {
	fn resolve(&self, name: openidconnect::reqwest::dns::Name) -> Resolving {
		let resolver = self.resolver.clone();
		Box::pin(async move {
			let lookup = resolver.lookup_ip(name.as_str()).await?;
			let addrs: Addrs = Box::new(SocketAddrs {
				iter: lookup.iter().collect::<Vec<_>>().into_iter(),
			});
			Ok(addrs)
		})
	}
}
