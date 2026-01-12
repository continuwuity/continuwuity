mod tasks;

use cargo_metadata::MetadataCommand;
use clap::Parser;

use crate::tasks::Task;

#[derive(clap::Parser)]
struct BaseArgs {
	#[command(subcommand)]
	task: Task,
	#[command(flatten)]
	args: Args,
}

#[derive(clap::Args)]
struct Args {
	/// Simulate without actually touching the filesystem
	#[arg(long)]
	dry_run: bool,
}

fn main() -> impl std::process::Termination {
	let BaseArgs { task, args } = BaseArgs::parse();

	let metadata = MetadataCommand::new()
		.no_deps()
		.exec()
		.expect("should have been able to run cargo");

	task.invoke(metadata, args)
}
