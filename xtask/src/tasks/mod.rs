type TaskResult<T> = Result<T, Box<dyn std::error::Error>>;

#[macro_export]
macro_rules! tasks {
    (
        $(
            $module:ident: $desc:literal
        ),*
    ) => {
        $(pub(super) mod $module;)*

        #[derive(clap::Subcommand)]
        #[allow(non_camel_case_types)]
        pub(super) enum Task {
            $(
                #[clap(about = $desc, long_about = None)]
                $module($module::Args),
            )*
        }

        impl Task {
            pub(super) fn invoke(self, common_args: $crate::Args) -> TaskResult<impl std::process::Termination> {
                match self {
                    $(
                        Self::$module(task_args) => {
                            $module::run(common_args, task_args)
                        },
                    )*
                }
            }
        }
    };
}

tasks! {
	generate_docs: "Generate various documentation files. This is run automatically when compiling the website."
}
