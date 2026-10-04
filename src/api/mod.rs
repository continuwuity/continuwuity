#![type_length_limit = "16384"] //TODO: reduce me
#![recursion_limit = "256"] // My Giant Async Function
#![allow(clippy::toplevel_ref_arg)]

extern crate continuwuity_core as continuwuity;
extern crate continuwuity_service as service;

continuwuity_macros::introspect_crate! {}

pub mod client;
pub mod router;
pub mod server;

pub mod client_ip;

pub mod admin;

pub(crate) use self::router::{Ruma, RumaResponse, State};

continuwuity::mod_ctor! {}
continuwuity::mod_dtor! {}
