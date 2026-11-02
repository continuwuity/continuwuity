mod commands;

use clap::Subcommand;
use conduwuit::Result;

use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum DebugCommand {
	/// Echo input of admin command
	Echo {
		message: Vec<String>,
	},

	/// Print extended memory usage
	///
	/// Optional argument is a character mask (a sequence of characters in any
	/// order) which enable additional extended statistics. Known characters are
	/// "abdeglmx". For convenience, a '*' will enable everything.
	MemoryStats {
		opts: Option<String>,
	},

	/// Print general tokio runtime metric totals.
	RuntimeMetrics,

	/// Print detailed tokio runtime metrics accumulated since last command
	///   invocation.
	RuntimeInterval,

	/// Get database statistics
	DatabaseStats {
		property: Option<String>,

		#[arg(short, long, alias("column"))]
		map: Option<String>,
	},

	/// Trim memory usage
	TrimMemory,

	/// List database files
	DatabaseFiles {
		map: Option<String>,

		#[arg(long)]
		level: Option<i32>,
	},

	/// Send a test email to the invoking admin's email address
	SendTestEmail,

	/// Lists room IDs by forward extremity count in descending order
	RoomsByExtremityCount {
		page: Option<usize>,
	},
}
