mod components;
pub(super) mod index;
pub(super) mod password_reset;
pub(super) mod resources;

#[derive(Debug)]
pub(crate) struct TemplateContext {
	pub allow_indexing: bool,
}

impl From<&crate::State> for TemplateContext {
	fn from(state: &crate::State) -> Self {
		Self {
			allow_indexing: state.config.index_page_allow_indexing,
		}
	}
}

#[macro_export]
macro_rules! template {
    (
        struct $name:ident $(<$lifetime:lifetime>)? use $path:literal {
            $($field_name:ident: $field_type:ty),*
        }
    ) => {
        #[derive(Debug, askama::Template)]
        #[template(path = $path)]
        struct $name$(<$lifetime>)? {
            context: $crate::pages::TemplateContext,
            $($field_name: $field_type,)*
        }

        impl$(<$lifetime>)? $name$(<$lifetime>)? {
            fn new(state: &$crate::State, $($field_name: $field_type,)*) -> Self {
                Self {
                    context: state.into(),
                    $($field_name,)*
                }
            }
        }

        #[allow(single_use_lifetimes)]
        impl$(<$lifetime>)? axum::response::IntoResponse for $name$(<$lifetime>)? {
            fn into_response(self) -> axum::response::Response {
                match self.render() {
                    Ok(rendered) => axum::response::Html(rendered).into_response(),
                    Err(err) => $crate::WebError::from(err).into_response()
                }
            }
        }
    };
}
