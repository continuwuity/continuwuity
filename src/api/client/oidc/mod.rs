/// OIDC
///
/// Stands for OpenID Connect, and is an authentication scheme relying on OAuth2.
/// The MSC2964 Matrix Spec Proposal describes an authentication process based
/// on OIDC with restrictions. See the [sample flow] for details on what's expected.
///
/// This module implements the needed endpoints. It relies on [`service::oidc`] and
/// the [oxide-auth] crate.
///
/// [oxide-auth]: https://docs.rs/oxide-auth
/// [sample flow]: https://github.com/sandhose/matrix-spec-proposals/blob/msc/sandhose/oauth2-profile/proposals/2964-oauth2-profile.md#sample-flow

mod discovery;
mod login;
mod authorize;
mod token;

pub(crate) use self::discovery::get_auth_metadata;
pub(crate) use self::login::oidc_login;
pub(crate) use self::authorize::{authorize, authorize_consent};
pub(crate) use self::token::token;
