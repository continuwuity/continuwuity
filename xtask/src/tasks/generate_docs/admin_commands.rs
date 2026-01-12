//! Generates documentation for the various commands that may be used in the admin room and server console.
//!
//! This generates one index page and several category pages, one for each of the direct subcommands of the top-level
//! `!admin` command. Those category pages then list all of the sub-subcommands.

use std::path::Path;

use askama::Template;
use clap::{Command, CommandFactory};
use conduwuit_admin::AdminCommand;

use crate::tasks::{TaskResult, generate_docs::FileOutput};

#[derive(askama::Template)]
#[template(path = "admin/index.md")]
/// The template for the index page, which links to all of the category pages.
struct Index {
    categories: Vec<Category>
}

/// A direct subcommand of the top-level `!admin` command.
#[derive(askama::Template)]
#[template(path = "admin/category.md")]
struct Category {
    name: String,
    description: String,
    commands: Vec<Subcommand>,
}

/// A second-or-deeper level subcommand of the `!admin` command.
struct Subcommand {
    name: String,
    description: String,
    /// How deeply nested this command was in the original command tree.
    /// This determines the header size used for it in the documentation.
    depth: usize,
}


fn flatten_subcommands(command: &Command) -> Vec<Subcommand> {

    fn flatten(
        subcommands: &mut Vec<Subcommand>,
        name_stack: &mut Vec<String>,
        command: &Command
    ) {
        let depth = name_stack.len();
        name_stack.push(command.get_name().to_owned());

        // do not include the root command
        if depth > 0 {
            let name = name_stack.join(" ");

            let description = command
                .get_long_about()
                .or_else(|| command.get_about())
                .map_or_else(|| "_(no description)_".to_owned(), ToString::to_string);

            subcommands.push(
                Subcommand {
                    name,
                    description,
                    depth,
                }
            );
        }

        for command in command.get_subcommands() {
            flatten(subcommands, name_stack, command);
        }

        name_stack.pop();
    }

    let mut subcommands = Vec::new();
    let mut name_stack = Vec::new();

    flatten(&mut subcommands, &mut name_stack, command);

    subcommands
}

pub(super) fn generate(out: &mut impl FileOutput) -> TaskResult<()> {
    let admin_commands = AdminCommand::command();

    let categories: Vec<_> = admin_commands
        .get_subcommands()
        .map(|command| {
            Category {
                name: command.get_name().to_owned(),
                description: command.get_about().expect("categories should have a docstring").to_string(),
                commands: flatten_subcommands(command),
            }
        })
        .collect();

    let root = Path::new("reference/admin/");

    for category in &categories {
        out.create_file(
            root.join(&category.name).with_extension("md"),
            category.render()?
        );
    }

    out.create_file(
        root.join("index.md"),
        Index { categories }.render()?,
    );

    Ok(())
}
