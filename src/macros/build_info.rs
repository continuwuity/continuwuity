use proc_macro2::TokenStream;
use quote::quote;

use crate::Result;

pub(super) fn introspect(_args: TokenStream) -> Result<TokenStream> {
	let cargo_crate_name = std::env::var("CARGO_CRATE_NAME").unwrap();
	let crate_name = cargo_crate_name.trim_start_matches("conduwuit_");
	let is_core = cargo_crate_name == "conduwuit_core";

	let flags = std::env::args().collect::<Vec<_>>();

	let mut enabled_features = Vec::new();
	append_features(&mut enabled_features, flags);

	let enabled_count = enabled_features.len();

	let import_path = if is_core {
		quote! { use crate::conduwuit_core; }
	} else {
		quote! { use ::conduwuit_core; }
	};

	let ret = quote! {
		#[doc(hidden)]
		mod __compile_introspection {
			#import_path

			/// Features that were enabled when this crate was compiled
			const ENABLED: [&str; #enabled_count] = [#( #enabled_features ),*];

			const CRATE_NAME: &str = #crate_name;

			/// Register this crate's features with the global registry during static initialization
			#[::ctor::ctor]
			fn register() {
				conduwuit_core::info::introspection::ENABLED_FEATURES.lock().unwrap().insert(#crate_name, &ENABLED);
			}
			#[::ctor::dtor]
			fn unregister() {
				conduwuit_core::info::introspection::ENABLED_FEATURES.lock().unwrap().remove(#crate_name);
			}
		}
	};

	Ok(ret)
}

fn append_features(features: &mut Vec<String>, flags: Vec<String>) {
	let mut next_is_cfg = false;
	for flag in flags {
		let is_cfg = flag == "--cfg";
		let is_feature = flag.starts_with("feature=");
		if std::mem::replace(&mut next_is_cfg, is_cfg) && is_feature {
			if let Some(feature) = flag
				.split_once('=')
				.map(|(_, feature)| feature.trim_matches('"'))
			{
				features.push(feature.to_owned());
			}
		}
	}
}
