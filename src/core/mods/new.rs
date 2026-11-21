use std::ffi::OsStr;

use super::{Library, path};
use crate::{Err, Result};

const OPEN_FLAGS: i32 = libloading::os::unix::RTLD_LAZY | libloading::os::unix::RTLD_GLOBAL;

/// # Safety
/// Loads a shared library via `dlopen(3)`, which executes the library's
/// initialization routines (`.init_array` / constructors). The caller must
/// ensure the resolved module is trusted and ABI-compatible with this binary.
pub unsafe fn from_name(name: &str) -> Result<Library> {
	let path = path::from_name(name)?;
	// SAFETY: Upheld by caller.
	unsafe { from_path(&path) }
}

/// # Safety
/// Loads a shared library via `dlopen(3)`, which executes the library's
/// initialization routines (`.init_array` / constructors). The caller must
/// ensure `path` points to a trusted, ABI-compatible shared library.
pub unsafe fn from_path(path: &OsStr) -> Result<Library> {
	// SAFETY: Caller guarantees `path` is a trusted, ABI-compatible library.
	let lib = unsafe { Library::open(Some(path), OPEN_FLAGS) };
	if let Err(e) = lib {
		let name = path::to_name(path)?;
		return Err!("Loading module {name:?} failed: {e}");
	}

	Ok(lib.expect("module loaded"))
}
