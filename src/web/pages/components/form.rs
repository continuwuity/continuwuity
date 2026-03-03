use askama::{Template, filters::HtmlSafe};
use validator::ValidationErrors;

/// A reusable form component with field validation.
#[derive(Debug, Template)]
#[template(path = "_components/form.html.j2", print = "code")]
pub(crate) struct Form<'a> {
	pub inputs: Vec<FormInput<'a>>,
	pub validation_errors: Option<ValidationErrors>,
	pub submit_label: &'a str,
}

impl HtmlSafe for Form<'_> {}

/// An input element in a form component.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FormInput<'a> {
	/// The field name of the input.
	pub id: &'static str,
	/// The `type` property of the input.
	pub input_type: &'a str,
	/// The contents of the input's label.
	pub label: &'a str,
	/// Whether the input is required. Defaults to `true`.
	pub required: bool,
	/// The autocomplete mode for the input. Defaults to `on`.
	pub autocomplete: &'a str,

	// This is a hack to make the form! macro's support for client-only fields
	// work properly. Client-only fields are specified in the macro without a type and aren't
	// included in the POST body or as a field in the generated struct.
	// To keep the field from being included in the POST body, its `name` property needs not to
	// be set in the template. Because of limitations of macro_rules!'s repetition feature, this
	// field needs to exist to allow the template to check if the field is client-only.
	#[doc(hidden)]
	pub type_name: Option<&'static str>,
}

impl Default for FormInput<'_> {
	fn default() -> Self {
		Self {
			id: "",
			input_type: "text",
			label: "",
			required: true,
			autocomplete: "",

			type_name: None,
		}
	}
}

/// Generate a deserializable struct which may be turned into a [`Form`]
/// for inclusion in another template.
#[macro_export]
macro_rules! form {
    (
        $(#[$struct_meta:meta])*
        struct $struct_name:ident {
            $(
                $(#[$field_meta:meta])*
                $name:ident$(: $type:ty)? where { $($prop:ident: $value:expr),* }
            ),*

            submit: $submit_label:expr
        }
    ) => {
        #[derive(Debug, serde::Deserialize, validator::Validate)]
        $(#[$struct_meta])*
        struct $struct_name {
            $(
                $(#[$field_meta])*
                $(pub $name: $type,)?
            )*
        }

        impl $struct_name {
            /// Generate a [`Form`] which matches the shape of this struct.
            #[allow(clippy::needless_update)]
            fn build(validation_errors: Option<validator::ValidationErrors>) -> $crate::pages::components::form::Form<'static> {
                $crate::pages::components::form::Form {
                    inputs: vec![
                        $(
                            $crate::pages::components::form::FormInput {
                                id: stringify!($name),
                                $(type_name: Some(stringify!($type)),)?
                                $($prop: $value),*,
                                ..Default::default()
                            },
                        )*
                    ],
                    validation_errors,
                    submit_label: $submit_label,
                }
            }
        }
    };
}
