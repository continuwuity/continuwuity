use std::{env, process::Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let mut child = Command::new("cargo").args(["run", "--package", "xtask-generate-commands", "--"].into_iter().map(ToOwned::to_owned).chain(env::args().skip(2)))
    // .stdout(Stdio::piped())
    // .stderr(Stdio::piped())
    .spawn()
    .expect("failed to execute child");
	child.wait()?;
	Ok(())
}
