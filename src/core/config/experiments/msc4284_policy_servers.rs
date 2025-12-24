use conduwuit_macros::config_example_generator;
use serde::Deserialize;

fn true_fn() -> bool { true }

fn default_federation_timeout() -> u64 { 25 }

#[derive(Clone, Debug, Default, Deserialize)]
#[config_example_generator(
	filename = "conduwuit-example.toml",
	section = "global.experiments.msc4284"
)]
pub struct MSC4248 {
	/// Enable or disable making requests to MSC4284 Policy Servers.
	/// It is recommended you keep this enabled unless you experience frequent
	/// connectivity issues, such as in a restricted networking environment.
	///
	/// default: true
	/// Introduced in: 0.5.0
	#[serde(default = "true_fn")]
	pub enabled: bool,

	/// Enable running locally generated events through configured MSC4284
	/// policy servers. You may wish to disable this if your server is
	/// single-user for a slight speed benefit in some rooms, but otherwise
	/// should leave it enabled.
	///
	/// If the room's policy server configuration requires event signatures,
	/// this option is effectively ignored, as otherwise local events would
	/// be rejected for missing the policy server's signature.
	///
	/// default: true
	/// Introduced in: 0.5.0
	#[serde(default = "true_fn")]
	pub check_own_events: bool,

	/// MSC4284 Policy server request timeout (seconds). Generally policy
	/// servers should respond near instantly, however may slow down under
	/// load. If a policy server doesn't respond in a short amount of time, the
	/// room it is configured in may become unusable if this limit is set too
	/// high. 25 seconds is a good default, however should be raised if you
	/// experience too many connection issues.
	///
	/// Please be aware that policy requests are *NOT* currently re-tried, so if
	/// a spam check request fails, the event will be assumed to be not spam,
	/// which in some cases may result in spam being sent to or received from
	/// the room that would typically be prevented.
	///
	/// If your request timeout is too low, and the policy server requires
	/// signatures, you may find that you are unable to send events that are
	/// accepted regardless.
	///
	/// About policy servers: https://matrix.org/blog/2025/04/introducing-policy-servers/
	/// default: 25
	/// Introduced in: 0.5.0
	#[serde(default = "default_federation_timeout")]
	pub request_timeout: u64,
}
