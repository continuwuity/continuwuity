use proc_macro::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{FnArg::Typed, Ident, ItemFn, Meta, Pat, PatIdent, PatType, Stmt};

use crate::Result;

pub(super) fn refutable(mut item: ItemFn, _args: &[Meta]) -> Result<TokenStream> {
	let inputs = item.sig.inputs.clone();
	let stmt = &mut item.block.stmts;
	let sig = &mut item.sig;
	for (i, input) in inputs.iter().enumerate() {
		let Typed(PatType { pat, .. }) = input else {
			continue;
		};

		let Pat::Struct(ref pat) = **pat else {
			continue;
		};

		let variant = &pat.path;
		let fields = &pat.fields;

		let Some(Typed(PatType { pat, .. })) = sig.inputs.get_mut(i) else {
			continue;
		};

		let name = format!("_args_{i}");
		**pat = Pat::Ident(PatIdent {
			ident: Ident::new(&name, Span::call_site().into()),
			attrs: Vec::new(),
			by_ref: None,
			mutability: None,
			subpat: None,
		});

		let field = fields.iter();
		let refute = quote! {
			let #variant { #( #field ),*, .. } = #name else { panic!("incorrect variant passed to function argument {i}"); };
		};

		stmt.insert(0, syn::parse2::<Stmt>(refute)?);
	}

	Ok(item.into_token_stream().into())
}
