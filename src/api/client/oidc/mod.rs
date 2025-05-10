//! OIDC
//!
//! Stands for OpenID Connect, and is an authentication scheme relying on
//! OAuth2. The [MSC2964] Matrix Spec Proposal describes an authentication
//! process based on the OIDC flow, with restrictions. See the [sample flow] for
//! details on what's expected.
//!
//! This module implements the needed endpoints. It relies on the [oxide-auth]
//! crate, and the [`service::oidc`] and [`web::oidc`] modules.
//!
//! [MSC2964]: https://github.com/matrix-org/matrix-spec-proposals/pull/2964
//! [oxide-auth]: https://docs.rs/oxide-auth
//! [sample flow]: https://github.com/sandhose/matrix-spec-proposals/blob/msc/sandhose/oauth2-profile/proposals/2964-oauth2-profile.md#sample-flow

mod authorize;
mod discovery;
mod login;
mod register;
mod token;

pub(crate) use self::{
	authorize::{authorize, authorize_consent},
	discovery::get_auth_metadata,
	login::oidc_login,
	register::register_client,
	token::token,
};
