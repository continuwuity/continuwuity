use std::{
	net::{IpAddr, SocketAddr},
	sync::Arc,
};

use hickory_resolver::{Resolver, net::runtime::TokioRuntimeProvider};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

/// A simple struct to implement reqwest::dns::Resolve, as that is what's
/// required by the reqwest client builder. Most code is based on
/// reqwest::dns::HickoryDnsResolver, which we here have to almost re-create
/// because it's pub(crate) instead of pub:
pub(crate) struct DnsResolver {
	resolver: Arc<Resolver<TokioRuntimeProvider>>,
}

impl DnsResolver {
	pub(crate) fn new(resolver: Arc<Resolver<TokioRuntimeProvider>>) -> Self { Self { resolver } }
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

impl Resolve for DnsResolver {
	fn resolve(&self, name: Name) -> Resolving {
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
