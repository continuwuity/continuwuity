mod commands;

use clap::Subcommand;
use continuwuity::Result;
use continuwuity_macros::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum OidcCommand {
	/// Link a user ID to the given subject claim.
	#[clap(name = "link")]
	OidcLink {
		user_id: String,
		subject: String,
	},

	/// Unlink the given subject claim from its associated user ID.
	#[clap(name = "unlink")]
	OidcUnlink {
		subject: String,
	},
}
