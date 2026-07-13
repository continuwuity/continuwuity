#![type_length_limit = "16384"] //TODO: reduce me
#![recursion_limit = "256"] // My Giant Async Function
#![allow(clippy::toplevel_ref_arg)]

extern crate conduwuit_core as conduwuit;
extern crate conduwuit_service as service;

conduwuit_macros::introspect_crate! {}

pub mod client;
pub mod router;
pub mod server;

pub mod client_ip;

pub mod admin;

pub(crate) use self::router::{Ruma, RumaResponse, State};

conduwuit::mod_ctor! {}
conduwuit::mod_dtor! {}
