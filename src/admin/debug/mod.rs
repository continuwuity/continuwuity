mod commands;
pub(crate) mod tester;

use clap::Subcommand;
use conduwuit::Result;
use ruma::{OwnedEventId, OwnedServerName};

use self::tester::TesterCommand;
use crate::admin_command_dispatch;

#[admin_command_dispatch]
#[derive(Debug, Subcommand)]
pub enum DebugCommand {
	/// Echo input of admin command
	Echo {
		message: Vec<String>,
	},

	/// Parse and print a PDU from a JSON
	///
	/// The PDU event is only checked for validity and is not added to the
	/// database.
	///
	/// This command needs a JSON blob provided in a Markdown code block below
	/// the command.
	ParsePdu,

	/// Forces device lists for all local and remote users to be updated (as
	///   having new keys available)
	ForceDeviceListUpdates,

	/// Change tracing log level/filter on the fly
	///
	/// This accepts the same format as the `log` config option.
	ChangeLogLevel {
		/// Log level/filter
		filter: Option<String>,

		/// Resets the log level/filter to the one in your config
		#[arg(short, long)]
		reset: bool,
	},

	/// Verify JSON signatures
	///
	/// This command needs a JSON blob provided in a Markdown code block below
	/// the command.
	VerifyJson,

	/// Verify PDU
	///
	/// This re-verifies a PDU existing in the database found by ID.
	VerifyPdu {
		event_id: OwnedEventId,
	},

	/// Runs a server name through Continuwuity's true destination resolution
	///   process
	///
	/// Useful for debugging well-known issues
	ResolveTrueDestination {
		server_name: OwnedServerName,

		#[arg(short, long)]
		no_cache: bool,
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

	/// Print the current time
	Time,

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

	/// Developer test stubs
	#[command(subcommand)]
	#[allow(non_snake_case)]
	#[clap(hide(true))]
	Tester(TesterCommand),
}
