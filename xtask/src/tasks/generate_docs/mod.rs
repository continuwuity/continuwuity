mod admin_commands;

use std::{collections::HashMap, path::{Path, PathBuf}};

use cargo_metadata::MetadataCommand;

use crate::tasks::TaskResult;

trait FileOutput {
    fn create_file(&mut self, path: PathBuf, contents: String);
}

#[derive(Default)]
struct FileQueue {
    queue: HashMap<PathBuf, String>,
}

impl FileQueue {
    fn write(self, root: &Path, dry_run: bool) -> std::io::Result<()> {
        for (path, contents) in self.queue.into_iter() {
            let path = root.join(&path);

            eprintln!("Writing {}", path.display());
            if !dry_run {
                std::fs::write(path, contents)?;
            }
        }

        Ok(())
    }
}

impl FileOutput for FileQueue {
    fn create_file(&mut self, path: PathBuf, contents: String) {
        assert!(path.is_relative(), "path must be relative");
        assert!(path.extension().is_some(), "path must not point to a directory");

        if self.queue.contains_key(&path) {
            panic!("attempted to create an already created file {}", path.display());
        }

        self.queue.insert(path, contents);
    }
}

#[derive(clap::Args)]
pub(crate) struct Args {
    /// The base path of the documentation. Defaults to `docs/` in the crate root.
    root: Option<PathBuf>,
}

pub(super) fn run(common_args: crate::Args, task_args: Args) -> TaskResult<()> {
    let mut queue = FileQueue::default();

    let metadata = MetadataCommand::new()
        .no_deps()
        .exec()
        .expect("should have been able to run cargo");

    let root = task_args.root.unwrap_or_else(|| metadata.workspace_root.join_os("docs/"));

    admin_commands::generate(&mut queue)?;

    queue.write(&root, common_args.dry_run)?;

    Ok(())
}
