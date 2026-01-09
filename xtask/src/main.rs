mod tasks;

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

	task.invoke(args)
}
