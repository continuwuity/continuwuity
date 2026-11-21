use std::{
	ffi::{CString, OsString},
	time::SystemTime,
};

use super::{Library, Symbol, canary, new, path};
use crate::{Result, error};

pub struct Module {
	handle: Option<Library>,
	loaded: SystemTime,
	path: OsString,
}

impl Module {
	/// # Safety
	/// See [`new::from_name`]: loads a shared library via `dlopen(3)` and runs
	/// its initialization routines.
	pub unsafe fn from_name(name: &str) -> Result<Self> {
		// SAFETY: Upheld by caller.
		unsafe { Self::from_path(path::from_name(name)?) }
	}

	/// # Safety
	/// See [`new::from_path`]: loads a shared library via `dlopen(3)` and runs
	/// its initialization routines.
	pub unsafe fn from_path(path: OsString) -> Result<Self> {
		Ok(Self {
			// SAFETY: Upheld by caller.
			handle: Some(unsafe { new::from_path(&path)? }),
			loaded: SystemTime::now(),
			path,
		})
	}

	pub fn unload(&mut self) {
		canary::prepare();
		self.close();
		if !canary::check_and_reset() {
			let name = self.name().expect("Module is named");
			error!("Module {name:?} is stuck and failed to unload.");
		}
	}

	pub(crate) fn close(&mut self) {
		if let Some(handle) = self.handle.take() {
			handle.close().expect("Module handle closed");
		}
	}

	/// Look up a symbol in this module via `dlsym(3)`.
	///
	/// # Safety
	/// `Prototype` is not checked against the symbol's actual type or ABI in
	/// the loaded shared library. The caller must ensure `name` refers to a
	/// symbol whose type, calling convention, and ABI exactly match
	/// `Prototype`.
	pub unsafe fn get<Prototype>(&self, name: &str) -> Result<Symbol<'_, Prototype>> {
		let cname = CString::new(name.to_owned()).expect("terminated string from provided name");
		let handle = self
			.handle
			.as_ref()
			.expect("backing library loaded by this instance");

		// SAFETY: Caller guarantees `Prototype` matches the symbol's actual
		// type and ABI. Passing `as_bytes_with_nul()` avoids a redundant copy
		// inside `libloading`.
		let sym = unsafe { handle.get::<Prototype>(cname.as_bytes_with_nul()) };
		let sym = sym.expect("symbol found; binding successful");

		Ok(sym)
	}

	pub fn deleted(&self) -> Result<bool> {
		let mtime = path::mtime(self.path())?;
		let res = mtime > self.loaded;

		Ok(res)
	}

	pub fn name(&self) -> Result<String> { path::to_name(self.path()) }

	#[must_use]
	pub fn path(&self) -> &OsString { &self.path }
}

impl Drop for Module {
	fn drop(&mut self) {
		if self.handle.is_some() {
			self.unload();
		}
	}
}
