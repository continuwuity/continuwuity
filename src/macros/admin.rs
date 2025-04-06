use itertools::Itertools;
use proc_macro::{Span, TokenStream};
use proc_macro2::TokenStream as TokenStream2;
use quote::{ToTokens, quote};
use syn::{Attribute, Error, Fields, Ident, ItemEnum, ItemFn, Meta, Variant, parse_quote};

use crate::{Result, utils::camel_to_snake_string};

pub(super) fn command(mut item: ItemFn, _args: &[Meta]) -> Result<TokenStream> {
	let attr: Attribute = parse_quote! {
		#[conduwuit_macros::implement(crate::Context, params = "<'_>")]
	};

	item.attrs.push(attr);
	Ok(item.into_token_stream().into())
}

pub(super) fn command_dispatch(item: ItemEnum, _args: &[Meta]) -> Result<TokenStream> {
	let name = &item.ident;
	let arm: Vec<TokenStream2> = item.variants.iter().map(dispatch_arm).try_collect()?;
	let switch = quote! {
		#[allow(clippy::large_stack_frames)] //TODO: fixme
		pub(super) async fn process(
			command: #name,
			context: &crate::Context<'_>
		) -> Result {
			use #name::*;
			#[allow(non_snake_case)]
			match command {
				#( #arm )*
			}
		}
	};

	Ok([item.into_token_stream(), switch]
		.into_iter()
		.collect::<TokenStream2>()
		.into())
}

fn dispatch_arm(v: &Variant) -> Result<TokenStream2> {
	let name = &v.ident;
	let target = camel_to_snake_string(&format!("{name}"));
	let handler = Ident::new(&target, Span::call_site().into());
	let res = match &v.fields {
		| Fields::Named(fields) => {
			let field = fields.named.iter().filter_map(|f| f.ident.as_ref());
			let arg = field.clone();
			quote! {
				#name { #( #field ),* } => {
					Box::pin(context.#handler(#( #arg ),*)).await
				},
			}
		},
		| Fields::Unnamed(fields) => {
			let Some(ref field) = fields.unnamed.first() else {
				return Err(Error::new(Span::call_site().into(), "One unnamed field required"));
			};
			quote! {
				#name ( #field ) => {
					Box::pin(#handler::process(#field, context)).await
				}
			}
		},
		| Fields::Unit => {
			quote! {
				#name => {
					Box::pin(context.#handler()).await
				},
			}
		},
	};

	Ok(res)
}
