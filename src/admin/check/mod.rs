mod commands;

use clap::Subcommand;
use conduwuit::Result;

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum CheckCommand {
	/// Uses the iterator in `src/database/key_value/users.rs` to iterator over
	/// every user in our database (remote and local). Reports total count, any
	/// errors if there were any, etc
	CheckAllUsers,
}
