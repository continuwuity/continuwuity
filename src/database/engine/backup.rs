use std::{ffi::OsString, path::PathBuf};

use conduwuit::{Err, Result, error, implement, info, utils::time::rfc2822_from_seconds, warn};
use rocksdb::backup::{BackupEngine, BackupEngineOptions};

use super::Engine;
use crate::util::map_err;

#[implement(Engine)]
#[tracing::instrument(skip(self), level = "info")]
pub fn backup(&self) -> Result {
	let mut engine = self.backup_engine()?;
	let config = &self.ctx.server.config;
	if config.database_backups_to_keep > 0 {
		engine
			.create_new_backup_flush(&self.db, true)
			.map_err(map_err)?;

		let engine_info = engine.get_backup_info();
		let info = &engine_info.last().expect("backup engine info is not empty");
		info!(
			"Created database backup #{} using {} bytes in {} files",
			info.backup_id, info.size, info.num_files,
		);
	}

	if config.database_backups_to_keep >= 0 {
		let keep = u32::try_from(config.database_backups_to_keep)?;
		if let Err(e) = engine.purge_old_backups(keep.try_into()?) {
			error!("Failed to purge old backup: {e:?}");
		}
	}

	if config.database_backups_to_keep == 0 {
		warn!("Configuration item `database_backups_to_keep` is set to 0.");
	}

	Ok(())
}

#[implement(Engine)]
pub fn backup_list(&self) -> Result<impl Iterator<Item = String> + Send> {
	let info = self.backup_engine()?.get_backup_info();

	if info.is_empty() {
		return Err!("No backups found.");
	}

	let list = info.into_iter().map(|info| {
		format!(
			"#{} {}: {} bytes, {} files",
			info.backup_id,
			rfc2822_from_seconds(info.timestamp),
			info.size,
			info.num_files,
		)
	});

	Ok(list)
}

#[implement(Engine)]
pub fn backup_count(&self) -> Result<usize> {
	let info = self.backup_engine()?.get_backup_info();

	Ok(info.len())
}

#[implement(Engine)]
fn backup_engine(&self) -> Result<BackupEngine> {
	let path = self.backup_path()?;
	let options = BackupEngineOptions::new(path).map_err(map_err)?;
	BackupEngine::open(&options, &self.ctx.env.lock()).map_err(map_err)
}

#[implement(Engine)]
fn backup_path(&self) -> Result<OsString> {
	let path = self
		.ctx
		.server
		.config
		.database_backup_path
		.clone()
		.map(PathBuf::into_os_string)
		.unwrap_or_default();

	if path.is_empty() {
		return Err!(Config("database_backup_path", "Configure path to enable backups"));
	}

	Ok(path)
}
