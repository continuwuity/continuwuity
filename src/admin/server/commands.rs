use std::{fmt::Write, path::PathBuf, sync::Arc};

use conduwuit::{
	Err, Result, info,
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
	self.write_str(&format!("{}", *self.services.server.config))
		.await
}

#[admin_command]
pub(super) async fn reload_config(&self, path: Option<PathBuf>) -> Result {
	let mut paths = Vec::new();
	if let Some(p) = path {
		paths.push(p);
	}
	self.services.config.reload(&paths)?;

	self.write_str("Successfully reconfigured.").await
}

#[admin_command]
pub(super) async fn list_features(&self, available: bool, enabled: bool, comma: bool) -> Result {
	let delim = if comma { "," } else { " " };
	if enabled && !available {
		let features = info::rustc::features().join(delim);
		let out = format!("`\n{features}\n`");
		return self.write_str(&out).await;
	}

	if available && !enabled {
		let features = info::cargo::features().join(delim);
		let out = format!("`\n{features}\n`");
		return self.write_str(&out).await;
	}

	let mut features = String::new();
	let enabled = info::rustc::features();
	let available = info::cargo::features();
	for feature in available {
		let active = enabled.contains(&feature.as_str());
		let emoji = if active { "✅" } else { "❌" };
		let remark = if active { "[enabled]" } else { "" };
		writeln!(features, "{emoji} {feature} {remark}")?;
	}

	self.write_str(&features).await
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
	warn!("shutdown command");
	self.services.server.shutdown()?;

	self.write_str("Shutting down server...").await
}
