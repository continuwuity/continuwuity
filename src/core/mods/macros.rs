#[macro_export]
macro_rules! mod_ctor {
	( $($body:block)? ) => {
		$crate::mod_init! {{
			$crate::debug_info!("Module loaded");
			$($body)?
		}}
	}
}

#[macro_export]
macro_rules! mod_dtor {
	( $($body:block)? ) => {
		$crate::mod_fini! {{
			$crate::debug_info!("Module unloading");
			$($body)?
			$crate::mods::canary::report();
		}}
	}
}

#[macro_export]
macro_rules! mod_init {
	($body:block) => {
		#[cfg(not(all(target_family = "unix", not(target_vendor = "apple"))))]
		compile_error!("conduwuit_mods (mod_init!) requires an ELF Unix target");

		#[used]
		// SAFETY: `.init_array` is the standard ELF constructor pointer table;
		// the dynamic loader invokes `_mod_init` before `main()` or on `dlopen`.
		#[cfg_attr(
			all(target_family = "unix", not(target_vendor = "apple")),
			unsafe(link_section = ".init_array")
		)]
		static MOD_INIT: unsafe extern "C" fn() = { _mod_init };

		// SAFETY: `.text.startup` is the standard ELF text subsection for
		// initialization functions.
		#[cfg_attr(
			all(target_family = "unix", not(target_vendor = "apple")),
			unsafe(link_section = ".text.startup")
		)]
		unsafe extern "C" fn _mod_init() -> () $body
	};
}

#[macro_export]
macro_rules! mod_fini {
	($body:block) => {
		#[cfg(not(all(target_family = "unix", not(target_vendor = "apple"))))]
		compile_error!("conduwuit_mods (mod_fini!) requires an ELF Unix target");

		#[used]
		// SAFETY: `.fini_array` is the standard ELF destructor pointer table;
		// the dynamic loader invokes `_mod_fini` on `dlclose` or process teardown.
		#[cfg_attr(
			all(target_family = "unix", not(target_vendor = "apple")),
			unsafe(link_section = ".fini_array")
		)]
		static MOD_FINI: unsafe extern "C" fn() = { _mod_fini };

		// SAFETY: `.text.exit` / `.text.startup` is a standard ELF text
		// subsection for teardown/initialization functions.
		#[cfg_attr(
			all(target_family = "unix", not(target_vendor = "apple")),
			unsafe(link_section = ".text.startup")
		)]
		unsafe extern "C" fn _mod_fini() -> () $body
	};
}
