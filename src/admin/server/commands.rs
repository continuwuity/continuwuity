use std::{fmt::Write, path::PathBuf, sync::Arc};

use conduwuit::{
	Err, Result,
	utils::{stream::IterStream, time},
	warn,
};
use futures::TryStreamExt;

use crate::admin_command;

#[admin_command]
pub(super) async fn uptime(&self) -> Result {
	let elapsed = self
		.services
		.server
		.started
		.elapsed()
		.expect("standard duration");

	let result = time::pretty(elapsed);
	self.write_str(&format!("{result}.")).await
}

#[admin_command]
pub(super) async fn show_config(&self) -> Result {
	self.bail_restricted()?;

	self.write_str(&format!("{}", *self.services.server.config))
		.await
}

#[admin_command]
pub(super) async fn reload_config(&self, path: Option<PathBuf>) -> Result {
	// The path argument is only what's optionally passed via the admin command,
	// so we need to merge it with the existing paths if any were given at startup.
	let mut paths = Vec::new();

	// Add previously saved paths to the argument list
	self.services
		.config
		.config_paths
		.clone()
		.unwrap_or_default()
		.iter()
		.for_each(|p| paths.push(p.to_owned()));

	// If a path is given, and it's not already in the list,
	// add it last, so that it overrides earlier files
	if let Some(p) = path {
		if !paths.contains(&p) {
			paths.push(p);
		}
	}

	self.services.config.reload(&paths)?;

	self.write_str(&format!("Successfully reconfigured from paths: {paths:?}"))
		.await
}

#[admin_command]
pub(super) async fn memory_usage(&self) -> Result {
	let services_usage = self.services.memory_usage().await?;
	let database_usage = self.services.db.db.memory_usage()?;
	let allocator_usage =
		conduwuit::alloc::memory_usage().map_or(String::new(), |s| format!("\nAllocator:\n{s}"));

	self.write_str(&format!(
		"Services:\n{services_usage}\nDatabase:\n{database_usage}{allocator_usage}",
	))
	.await
}

#[admin_command]
pub(super) async fn clear_caches(&self) -> Result {
	self.services.clear_cache().await;

	self.write_str("Done.").await
}

#[admin_command]
pub(super) async fn list_backups(&self) -> Result {
	self.services
		.db
		.db
		.backup_list()?
		.try_stream()
		.try_for_each(|result| write!(self, "{result}"))
		.await
}

#[admin_command]
pub(super) async fn backup_database(&self) -> Result {
	self.bail_restricted()?;

	let db = Arc::clone(&self.services.db);
	let result = self
		.services
		.server
		.runtime()
		.spawn_blocking(move || match db.db.backup() {
			| Ok(()) => "Done".to_owned(),
			| Err(e) => format!("Failed: {e}"),
		})
		.await?;

	let count = self.services.db.db.backup_count()?;
	self.write_str(&format!("{result}. Currently have {count} backups."))
		.await
}

#[admin_command]
pub(super) async fn admin_notice(&self, message: Vec<String>) -> Result {
	let message = message.join(" ");
	self.services.admin.send_text(&message).await;

	self.write_str("Notice was sent to #admins").await
}

#[admin_command]
pub(super) async fn reload_mods(&self) -> Result {
	self.bail_restricted()?;

	self.services.server.reload()?;

	self.write_str("Reloading server...").await
}

#[admin_command]
#[cfg(unix)]
pub(super) async fn restart(&self, force: bool) -> Result {
	use conduwuit::utils::sys::current_exe_deleted;

	if !force && current_exe_deleted() {
		return Err!(
			"The server cannot be restarted because the executable changed. If this is expected \
			 use --force to override."
		);
	}

	self.services.server.restart()?;

	self.write_str("Restarting server...").await
}

#[admin_command]
pub(super) async fn shutdown(&self) -> Result {
	self.bail_restricted()?;

	warn!("shutdown command");
	self.services.server.shutdown()?;

	self.write_str("Shutting down server...").await
}

#[admin_command]
pub(super) async fn list_features(&self) -> Result {
	let mut enabled_features = conduwuit::info::introspection::ENABLED_FEATURES
		.lock()
		.expect("locked")
		.iter()
		.flat_map(|(_, f)| f.iter())
		.collect::<Vec<_>>();

	enabled_features.sort_unstable();
	enabled_features.dedup();

	let mut available_features = conduwuit::build_metadata::WORKSPACE_FEATURES
		.iter()
		.flat_map(|(_, f)| f.iter())
		.collect::<Vec<_>>();

	available_features.sort_unstable();
	available_features.dedup();

	let mut features = String::new();

	for feature in available_features {
		let active = enabled_features.contains(&feature);
		let emoji = if active { "✅" } else { "❌" };
		let remark = if active { "[enabled]" } else { "" };
		writeln!(features, "{emoji} {feature} {remark}")?;
	}

	self.write_str(&features).await
}

#[admin_command]
pub(super) async fn build_info(&self) -> Result {
	use conduwuit::build_metadata::built;

	let mut info = String::new();

	// Version information
	writeln!(info, "# Build Information\n")?;
	writeln!(info, "**Version:** {}", built::PKG_VERSION)?;
	writeln!(info, "**Package:** {}", built::PKG_NAME)?;
	writeln!(info, "**Description:** {}", built::PKG_DESCRIPTION)?;

	// Git information
	writeln!(info, "\n## Git Information\n")?;
	if let Some(hash) = conduwuit::build_metadata::GIT_COMMIT_HASH {
		writeln!(info, "**Commit Hash:** {hash}")?;
	}
	if let Some(hash) = conduwuit::build_metadata::GIT_COMMIT_HASH_SHORT {
		writeln!(info, "**Commit Hash (short):** {hash}")?;
	}
	if let Some(url) = conduwuit::build_metadata::GIT_REMOTE_WEB_URL {
		writeln!(info, "**Repository:** {url}")?;
	}
	if let Some(url) = conduwuit::build_metadata::GIT_REMOTE_COMMIT_URL {
		writeln!(info, "**Commit URL:** {url}")?;
	}

	// Build environment
	writeln!(info, "\n## Build Environment\n")?;
	writeln!(info, "**Profile:** {}", built::PROFILE)?;
	writeln!(info, "**Optimization Level:** {}", built::OPT_LEVEL)?;
	writeln!(info, "**Debug:** {}", built::DEBUG)?;
	writeln!(info, "**Target:** {}", built::TARGET)?;
	writeln!(info, "**Host:** {}", built::HOST)?;

	// Rust compiler information
	writeln!(info, "\n## Compiler Information\n")?;
	writeln!(info, "**Rustc Version:** {}", built::RUSTC_VERSION)?;
	if !built::RUSTDOC_VERSION.is_empty() {
		writeln!(info, "**Rustdoc Version:** {}", built::RUSTDOC_VERSION)?;
	}

	// Target configuration
	writeln!(info, "\n## Target Configuration\n")?;
	writeln!(info, "**Architecture:** {}", built::CFG_TARGET_ARCH)?;
	writeln!(info, "**OS:** {}", built::CFG_OS)?;
	writeln!(info, "**Family:** {}", built::CFG_FAMILY)?;
	writeln!(info, "**Endianness:** {}", built::CFG_ENDIAN)?;
	writeln!(info, "**Pointer Width:** {} bits", built::CFG_POINTER_WIDTH)?;
	if !built::CFG_ENV.is_empty() {
		writeln!(info, "**Environment:** {}", built::CFG_ENV)?;
	}

	// CI information
	if let Some(ci) = built::CI_PLATFORM {
		writeln!(info, "\n## CI Platform\n")?;
		writeln!(info, "**Platform:** {ci}")?;
	}

	self.write_str(&info).await
}
