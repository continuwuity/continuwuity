mod msc4284_policy_servers;

use conduwuit_macros::config_example_generator;
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
#[config_example_generator(filename = "conduwuit-example.toml", section = "global.experiments")]
pub struct Experiments {
	/// Enforce MSC4311's updated requirements on all incoming invites.
	///
	/// This drastically increases the security and filtering capabilities
	/// when processing invites over federation, at the cost of compatibility.
	/// Servers that do not implement MSC4311 will be unable to send invites
	/// to your server when this is enabled, including continuwuity 0.5.0 and
	/// below.
	///
	/// default: false
	/// Introduced in: (unreleased)
	#[serde(default)]
	pub enforce_msc4311: bool,

	/// MSC4284 Policy Server support configuration.
	#[serde(default)]
	pub msc4284: msc4284_policy_servers::MSC4248,
}
