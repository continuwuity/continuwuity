use serde::Deserialize;

/// Configuration for experimental features.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct Experiments {
	/// MSC2815: Proposal to allow room moderators to view redacted event
	/// content
	///
	/// Allows the server to retain unredacted copies of events that have been
	/// redacted for a limited period of time, which allows room moderators and
	/// server administrators to reveal redacted contents. Useful for moderation
	/// and investigating reports.
	///
	/// Requires a supporting client to use.
	///
	/// https://github.com/matrix-org/matrix-spec-proposals/pull/2815
	#[serde(default)]
	pub msc2815: Option<MSC2815>,
	/// MSC4155: Invite filtering
	///
	/// Provides advanced invite filtering rules for users, allowing complex
	/// configurations that can allow, ignore, and reject invites from users and
	/// servers broadly.
	///
	/// Requires a supporting client to set up on an account.
	///
	/// https://github.com/matrix-org/matrix-spec-proposals/pull/4155
	#[serde(default)]
	pub msc4155: SimpleExperiment,
	/// MSC4174: Web push
	///
	/// Adds server-side support for web push, allowing web clients to receive
	/// push notifications directly without the need for an intermediary push
	/// gateway like Sygnal.
	///
	/// https://github.com/matrix-org/matrix-spec-proposals/pull/4174
	#[serde(default)]
	pub msc4174: SimpleExperiment,
	/// MSC4484: Server Administration OAuth Scope
	///
	/// Adds a new OAuth2 scope that explicitly grants a token temporary
	/// homeserver administrator privileges, granting temporary token access
	/// to administrator endpoints like locking & suspension.
	///
	/// https://github.com/matrix-org/matrix-spec-proposals/pull/4484
	#[serde(default)]
	pub msc4484: SimpleExperiment,
	/// MSC4494: Membership-based invite blocking
	///
	/// Adds a new invite blocking action that will reject all invites coming
	/// from users who do not share a room with the invitee that is not `public`
	/// (the join rule).
	///
	/// This is not related to MSC4155, this proposal instead builds on the
	/// "invite blocking" proposal that landed in the specification in v1.17,
	/// which simply allows enabling or disabling invites altogether.
	///
	/// MSC4494 is a good middle-ground between the allowlisting possible with
	/// MSC4155, and the general desire to block invites from users you may not
	/// know.
	///
	/// Requires a supporting client to enable on an account.
	///
	/// https://github.com/matrix-org/matrix-spec-proposals/pull/4494
	#[serde(default)]
	pub msc4494: SimpleExperiment,
	/// MSC4560: Stripped state requests over federation
	///
	/// Implements a more efficient way for servers to acquire and serve
	/// "stripped state" to clients, which is typically only used for displaying
	/// rooms. The display of space hierarchies and room summaries is
	/// significantly faster when this proposal is enabled and used.
	///
	/// https://github.com/matrix-org/matrix-spec-proposals/pull/4560
	#[serde(default)]
	pub msc4560: SimpleExperiment,
}

/// Represents a "simple" experiment, which is either disabled or enabled.
///
/// More complex experiments that require configuration should define their own
/// type and implement `ToggledExperiment` instead.
#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct SimpleExperiment(Option<bool>);

pub trait ToggledExperiment {
	/// Returns true if this experiment is explicitly enabled.
	#[must_use]
	fn is_enabled(&self) -> bool;
}

impl ToggledExperiment for SimpleExperiment {
	#[inline]
	fn is_enabled(&self) -> bool { self.0.is_some_and(crate::is_true!()) }
}

/// Configuration for [MSC2815]: Proposal to allow room moderators to view
/// redacted event content
///
/// MSC2815: https://github.com/matrix-org/matrix-spec-proposals/pull/2815
#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub struct MSC2815 {
	#[serde(default)]
	pub enabled: bool,
	#[serde(default = "default_unredacted_content_max_age_secs")]
	pub unredacted_content_max_age_secs: u64,
}

#[inline]
fn default_unredacted_content_max_age_secs() -> u64 { 7_776_000 }

impl ToggledExperiment for Option<MSC2815> {
	#[inline]
	fn is_enabled(&self) -> bool { self.is_some_and(|config| config.enabled) }
}
