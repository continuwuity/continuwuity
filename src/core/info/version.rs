//! one true function for returning the conduwuit version with the necessary
//! CONDUWUIT_VERSION_EXTRA env variables used if specified
//!
//! Set the environment variable `CONDUWUIT_VERSION_EXTRA` to any UTF-8 string
//! to include it in parenthesis after the SemVer version. A common value are
//! git commit hashes.

use std::sync::OnceLock;

static BRANDING: &str = "continuwuity";
static WEBSITE: &str = "https://continuwuity.org";
static SEMANTIC: &str = env!("CARGO_PKG_VERSION");

static VERSION: OnceLock<String> = OnceLock::new();
static VERSION_UA: OnceLock<String> = OnceLock::new();
static USER_AGENT: OnceLock<String> = OnceLock::new();
static USER_AGENT_MEDIA: OnceLock<String> = OnceLock::new();

#[inline]
#[must_use]
pub fn name() -> &'static str { BRANDING }

#[inline]
pub fn version() -> &'static str { VERSION.get_or_init(init_version) }

#[inline]
pub fn version_ua() -> &'static str { VERSION_UA.get_or_init(init_version_ua) }

#[inline]
pub fn user_agent() -> &'static str { USER_AGENT.get_or_init(init_user_agent) }

#[inline]
pub fn user_agent_media() -> &'static str { USER_AGENT_MEDIA.get_or_init(init_user_agent_media) }

fn init_user_agent() -> String { format!("{}/{} (bot; +{WEBSITE})", name(), version_ua()) }

fn init_user_agent_media() -> String {
	format!("{}/{} (embedbot; facebookexternalhit/1.1; +{WEBSITE})", name(), version_ua())
}

fn init_version_ua() -> String {
	conduwuit_build_metadata::version_tag()
		.map_or_else(|| SEMANTIC.to_owned(), |extra| format!("{SEMANTIC}+{extra}"))
}

fn init_version() -> String {
	conduwuit_build_metadata::version_tag()
		.map_or_else(|| SEMANTIC.to_owned(), |extra| format!("{SEMANTIC} ({extra})"))
}
