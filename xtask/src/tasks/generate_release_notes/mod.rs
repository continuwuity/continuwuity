use askama::Template;
use duct::cmd;

use crate::tasks::TaskResult;

#[derive(askama::Template)]
#[template(path = "release-notes.md")]
struct ReleaseNotes<'a> {
    version: &'a str,
    header: &'a str,
    changelog: &'a str,
}

#[derive(clap::Args)]
pub(crate) struct Args;

pub(super) fn run(_: cargo_metadata::Metadata, _: crate::Args, _: Args) -> TaskResult<()> {
    const TAG_PREFIX: &str = "v";

    let tag = cmd!("git", "describe", "--exact")
        .stdout_capture()
        .read()
        .expect("failed to get current tag");

    let version = tag
        .strip_prefix(TAG_PREFIX)
        .expect("tag did not start with expected prefix");

    eprintln!("Generating release notes for {version}");

    let header = cmd!("git", "tag", "-l", &tag, "--format", "%(contents)")
        .stdout_capture()
        .read()
        .expect("failed to read tag contents");

    let mut changelog = cmd!("towncrier", "build", "--draft", "--version", &version)
        .stdout_capture()
        .stderr_null()
        .read()
        .expect("failed to run towncrier");


    // towncrier generates a title with the project name and date, which we don't want. remove it here.
    // split off the first line and make sure it's a markdown heading
    assert!(changelog.starts_with("# "), "expected h1 at the start of towncrier output");
    let changelog = changelog.split_off(changelog.find("\n\n").unwrap());

    let rendered = ReleaseNotes {
        version,
        header: header.trim(),
        changelog: changelog.trim()
    }.render()?;

    println!("{rendered}");

    Ok(())
}
