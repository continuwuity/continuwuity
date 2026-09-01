use std::ffi::OsStr;

use super::{Library, path};
use crate::{Err, Result};

const OPEN_FLAGS: i32 = libloading::os::unix::RTLD_LAZY | libloading::os::unix::RTLD_GLOBAL;

pub fn from_name(name: &str) -> Result<Library> {
	let path = path::from_name(name)?;
	from_path(&path)
}

pub fn from_path(path: &OsStr) -> Result<Library> {
	//SAFETY: Calls dlopen(3) on unix platforms. This might not have to be
	// unsafe if wrapped in with_dlerror.
	let lib = unsafe { Library::open(Some(path), OPEN_FLAGS) };
	if let Err(e) = lib {
		let name = path::to_name(path)?;
		return Err!("Loading module {name:?} failed: {e}");
	}

	Ok(lib.expect("module loaded"))
}
