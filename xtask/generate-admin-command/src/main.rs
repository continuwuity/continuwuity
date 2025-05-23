use std::{
	fs::{self, File},
	io,
	path::Path,
};

use clap_builder::{Command, CommandFactory};
use conduwuit_admin::AdminCommand;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut args = std::env::args().skip(1);
	let task = args.next();
	match task {
		| None => todo!(),
		| Some(t) => match t.as_str() {
			| "man" => {
				let dir = Path::new("./admin-man");
				gen_manpages(dir)?;
			},
			| "md" => {
				let command = AdminCommand::command().name("admin");

				let res = clap_markdown::help_markdown_command_custom(
					&command,
					&clap_markdown::MarkdownOptions::default(),
				);

				println!("{res}");
			},
			| invalid => return Err(format!("Invalid task name: {invalid}").into()),
		},
	}
	Ok(())
}

fn gen_manpages(dir: &Path) -> Result<(), io::Error> {
	fn r#gen(dir: &Path, c: &Command, prefix: Option<&str>) -> Result<(), io::Error> {
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
			r#gen(&dir.join(sub_name), sub, Some(&name))?;
		}

		Ok(())
	}

	r#gen(dir, &AdminCommand::command().name("admin"), None)
}
