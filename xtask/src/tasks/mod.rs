type TaskResult<T> = Result<T, Box<dyn std::error::Error>>;

macro_rules! tasks {
    (
        $(
            $(#[$meta:meta])? $module:ident: $desc:literal
        ),*
    ) => {
        $(
            $(#[cfg($meta)])?
            pub(super) mod $module;
        )*

        #[derive(clap::Subcommand)]
        #[allow(non_camel_case_types)]
        pub(super) enum Task {
            $(
                $(#[cfg($meta)])?
                #[clap(about = $desc, long_about = None)]
                $module($module::Args),
            )*
        }

        impl Task {
            pub(super) fn invoke(self, metadata: cargo_metadata::Metadata, common_args: $crate::Args) -> TaskResult<impl std::process::Termination> {
                match self {
                    $(
                        $(#[cfg($meta)])?
                        Self::$module(task_args) => {
                            $module::run(metadata, common_args, task_args)
                        },
                    )*
                }
            }
        }
    };
}

tasks! {
	#[feature = "generate-docs"]
	generate_docs: "Generate various documentation files. This is run automatically when compiling the website.",
	generate_release_notes: "Generate release notes from towncrier and a git tag. Used by CI."
}
