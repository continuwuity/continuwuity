pub mod introspection;
pub mod room_version;
pub mod version;

pub const MODULE_ROOT: &str = const_str::split!(std::module_path!(), "::")[0];
pub const CRATE_PREFIX: &str = const_str::split!(MODULE_ROOT, '_')[0];
