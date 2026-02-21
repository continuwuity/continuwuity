use clap::Subcommand;
use conduwuit::{Err, Result, utils::time};
use futures::StreamExt;
use ruma::OwnedServerName;

use crate::{admin_command, admin_command_dispatch};

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
#[allow(clippy::enum_variant_names)]
/// Resolver service and caches
pub enum ResolverCommand {
	/// Query the destinations cache
	DestinationsCache {
		server_name: Option<OwnedServerName>,
	},

	/// Query the overrides cache
	OverridesCache {
		name: Option<String>,
	},

	/// Flush a given server from the resolver caches or flush them completely
	///
	/// * Examples:
	///   * Flush a specific server:
	///
	///     `!admin query resolver flush-cache matrix.example.com`
	///
	///   * Flush all resolver caches completely:
	///
	///     `!admin query resolver flush-cache --all`
	#[command(verbatim_doc_comment)]
	FlushCache {
		name: Option<OwnedServerName>,

		#[arg(short, long)]
		all: bool,
	},
}

#[admin_command]
async fn destinations_cache(&self, server_name: Option<OwnedServerName>) -> Result {
	use service::resolver::cache::CachedDest;

	writeln!(self, "| Server Name | Destination | Hostname | Expires |").await?;
	writeln!(self, "| ----------- | ----------- | -------- | ------- |").await?;

	let mut destinations = self.services.resolver.cache.destinations().boxed();

	while let Some((name, CachedDest { dest, host, expire })) = destinations.next().await {
		if let Some(server_name) = server_name.as_ref() {
			if name != server_name {
				continue;
			}
		}

		let expire = time::format(expire, "%+");
		self.write_str(&format!("| {name} | {dest} | {host} | {expire} |\n"))
			.await?;
	}

	Ok(())
}

#[admin_command]
async fn overrides_cache(&self, server_name: Option<String>) -> Result {
	use service::resolver::cache::CachedOverride;

	writeln!(self, "| Server Name | IP  | Port | Expires | Overriding |").await?;
	writeln!(self, "| ----------- | --- | ----:| ------- | ---------- |").await?;

	let mut overrides = self.services.resolver.cache.overrides().boxed();

	while let Some((name, CachedOverride { ips, port, expire, overriding })) =
		overrides.next().await
	{
		if let Some(server_name) = server_name.as_ref() {
			if name != server_name {
				continue;
			}
		}

		let expire = time::format(expire, "%+");
		self.write_str(&format!("| {name} | {ips:?} | {port} | {expire} | {overriding:?} |\n"))
			.await?;
	}

	Ok(())
}

#[admin_command]
async fn flush_cache(&self, name: Option<OwnedServerName>, all: bool) -> Result {
	if all {
		self.services.resolver.cache.clear().await;
		writeln!(self, "Resolver caches cleared!").await
	} else if let Some(name) = name {
		self.services.resolver.cache.del_destination(&name);
		self.services.resolver.cache.del_override(&name);
		self.write_str(&format!("Cleared {name} from resolver caches!"))
			.await
	} else {
		Err!("Missing name. Supply a name or use --all to flush the whole cache.")
	}
}
