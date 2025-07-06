use std::{
	fs::{self, File},
	io::{self, Write},
	path::Path,
};

use clap_builder::{Command, CommandFactory};
use conduwuit_admin::AdminCommand;

enum CommandType {
	Admin,
	Server,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut args = std::env::args().skip(1);
	let command_type = args.next();
	let task = args.next();

	match (command_type, task) {
		| (None, _) => {
			return Err("Missing command type (admin or server)".into());
		},
		| (Some(cmd_type), None) => {
			return Err(format!("Missing task for {cmd_type} command").into());
		},
		| (Some(cmd_type), Some(task)) => {
			let command_type = match cmd_type.as_str() {
				| "admin" => CommandType::Admin,
				| "server" => CommandType::Server,
				| _ => return Err(format!("Invalid command type: {cmd_type}").into()),
			};

			match task.as_str() {
				| "man" => match command_type {
					| CommandType::Admin => {
						let dir = Path::new("./admin-man");
						gen_admin_manpages(dir)?;
					},
					| CommandType::Server => {
						let dir = Path::new("./server-man");
						gen_server_manpages(dir)?;
					},
				},
				| "md" => {
					match command_type {
						| CommandType::Admin => {
							let command = AdminCommand::command().name("admin");

							let res = clap_markdown::help_markdown_command_custom(
								&command,
								&clap_markdown::MarkdownOptions::default().show_footer(false),
							)
							.replace("\n\r", "\n")
							.replace("\r\n", "\n")
							.replace(" \n", "\n");

							let mut file = File::create(Path::new("./docs/admin_reference.md"))?;
							file.write_all(res.trim_end().as_bytes())?;
							file.write_all(b"\n")?;
						},
						| CommandType::Server => {
							// Get the server command from the conduwuit crate
							let command = conduwuit::Args::command();

							let res = clap_markdown::help_markdown_command_custom(
								&command,
								&clap_markdown::MarkdownOptions::default().show_footer(false),
							)
							.replace("\n\r", "\n")
							.replace("\r\n", "\n")
							.replace(" \n", "\n");

							let mut file = File::create(Path::new("./docs/server_reference.md"))?;
							file.write_all(res.trim_end().as_bytes())?;
							file.write_all(b"\n")?;
						},
					}
				},
				| invalid => return Err(format!("Invalid task name: {invalid}").into()),
			}
		},
	}
	Ok(())
}

fn gen_manpage_common(dir: &Path, c: &Command, prefix: Option<&str>) -> Result<(), io::Error> {
	fs::create_dir_all(dir)?;
	let sub_name = c.get_display_name().unwrap_or_else(|| c.get_name());
	let name = if let Some(prefix) = prefix {
		format!("{prefix}-{sub_name}")
	} else {
		sub_name.to_owned()
	};

	let mut out = File::create(dir.join(format!("{name}.1")))?;
	let clap_mangen = clap_mangen::Man::new(c.to_owned().disable_help_flag(true));
	clap_mangen.render(&mut out)?;

	for sub in c.get_subcommands() {
		gen_manpage_common(&dir.join(sub_name), sub, Some(&name))?;
	}

	Ok(())
}

fn gen_admin_manpages(dir: &Path) -> Result<(), io::Error> {
	gen_manpage_common(dir, &AdminCommand::command().name("admin"), None)
}

fn gen_server_manpages(dir: &Path) -> Result<(), io::Error> {
	gen_manpage_common(dir, &conduwuit::Args::command(), None)
}
