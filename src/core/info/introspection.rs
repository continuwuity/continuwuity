//! Information about features the crates were compiled with.
//! Only available for crates that have called the `introspect_crate` macro

use std::collections::BTreeMap;

pub static ENABLED_FEATURES: std::sync::Mutex<BTreeMap<&str, &[&str]>> =
	std::sync::Mutex::new(BTreeMap::new());
