use clap::Subcommand;
use conduwuit::{Err, Result, utils::time};
use resolvematrix::resolution::Resolution;
use ruma::OwnedServerName;

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
#[allow(clippy::enum_variant_names)]
/// Resolver service and caches
pub enum ResolverCommand {
	/// Query the destinations or overrides cache, depending on the value of the
	/// `overrides` flag (default false)
	Cache {
		server_name: Option<OwnedServerName>,

		#[arg(short, long)]
		overrides: Option<bool>,
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

impl crate::Context<'_> {
	async fn cache(
		&self,
		server_name: Option<OwnedServerName>,
		overrides: Option<bool>,
	) -> Result {
		writeln!(self, "| Server Name | Destination | SNI | Override | Step | Expires |").await?;
		writeln!(self, "| ----------- | ----------- | --- | -------- | ---- | ------- |").await?;

		let entries = self.services.client.resolver.get_all_cache_entries();

		for (host, entry) in entries {
			if let Some(ors) = overrides
				&& entry.is_override != ors
			{
				continue;
			}

			if let Some(server_name) = server_name.as_ref()
				&& host != *server_name
			{
				continue;
			}

			let Resolution {
				destination,
				is_override,
				host: sni_host,
				resolution_step,
			} = entry.resolution;
			let expires = time::format(entry.expires_at, "%Y-%m-%dT%H:%M:%S%.3f%z");
			self.write_str(&format!(
				"| {host} | {destination:?} | {sni_host} | {is_override:?} | {resolution_step} \
				 | {expires} |\n"
			))
			.await?;
		}

		Ok(())
	}

	async fn flush_cache(&self, name: Option<OwnedServerName>, all: bool) -> Result {
		if all {
			self.services.client.resolver.clear_cache();
			self.services.client.dns.clear_cache();
			writeln!(self, "Resolver and DNS caches cleared!").await
		} else if let Some(name) = name {
			self.services
				.client
				.resolver
				.remove_cache_entry(name.as_str());
			self.write_str(&format!("Cleared {name} from resolver caches!"))
				.await
		} else {
			Err!("Missing name. Supply a name or use --all to flush the whole cache.")
		}
	}
}
