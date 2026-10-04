#![cfg(all(continuwuity_mods, feature = "continuwuity_mods"))]

pub(crate) use libloading::os::unix::{Library, Symbol};

pub mod canary;
pub mod macros;
pub mod module;
pub mod new;
pub mod path;

pub use module::Module;
