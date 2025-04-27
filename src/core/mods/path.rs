use std::{
	env::current_exe,
	ffi::{OsStr, OsString},
	path::{Path, PathBuf},
	time::SystemTime,
};

use libloading::library_filename;

use crate::Result;

pub fn from_name(name: &str) -> Result<OsString> {
	let root = PathBuf::new();
	let exe_path = current_exe()?;
	let exe_dir = exe_path.parent().unwrap_or(&root);
	let mut mod_path = exe_dir.to_path_buf();
	let mod_file = library_filename(name);
	mod_path.push(mod_file);

	Ok(mod_path.into_os_string())
}

pub fn to_name(path: &OsStr) -> Result<String> {
	let path = Path::new(path);
	let name = path
		.file_stem()
		.expect("path file stem")
		.to_str()
		.expect("name string");

	let name = name.strip_prefix("lib").unwrap_or(name).to_owned();

	Ok(name)
}

pub fn mtime(path: &OsStr) -> Result<SystemTime> {
	let meta = std::fs::metadata(path)?;
	let mtime = meta.modified()?;

	Ok(mtime)
}
