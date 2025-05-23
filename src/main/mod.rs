#![type_length_limit = "49152"] //TODO: reduce me

use conduwuit_core::rustc_flags_capture;

pub(crate) mod clap;
mod logging;
mod mods;
mod restart;
mod runtime;
mod sentry;
mod server;
mod signal;

rustc_flags_capture! {}
