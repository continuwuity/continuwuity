mod commands;

use clap::{Args, Subcommand};
use conduwuit::Result;

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum TokenCommand {
	/// Issue a new registration token
	#[clap(name = "issue")]
	IssueToken {
		/// When this token will expire.
		#[command(flatten)]
		expires: TokenExpires,
	},

	/// Revoke a registration token
	#[clap(name = "revoke")]
	RevokeToken {
		/// The token to revoke.
		token: String,
	},

	/// List all registration tokens
	#[clap(name = "list")]
	ListTokens,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = true)]
pub struct TokenExpires {
	/// The maximum number of times this token is allowed to be used before it
	/// expires.
	#[arg(long, conflicts_with_all = ["immortal", "once"])]
	max_uses: Option<u64>,

	/// The maximum age of this token (e.g. 30s, 5m, 7d). It will expire after
	/// this much time has passed.
	#[arg(long, conflicts_with_all = ["immortal", "once"])]
	max_age: Option<String>,

	/// This token will never expire.
	#[arg(long, conflicts_with_all = ["once", "max_uses", "max_age"])]
	immortal: bool,

	/// A shortcut for `--max-uses 1`.
	#[arg(long, conflicts_with_all = ["immortal", "max_uses", "max_age"])]
	once: bool,
}
