mod stencil;

use darling::{ast::NestedMeta, Error, FromMeta};
use proc_macro::TokenStream;
use syn::parse_macro_input;

use crate::stencil::{StencilFn, StencilMeta};

#[proc_macro_attribute]
pub fn stencil(attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as StencilFn);
    let err = match NestedMeta::parse_meta_list(attr.into()) {
        Ok(meta) => match StencilMeta::from_list(&meta) {
            Ok(meta) => return function.expand(meta),
            Err(err) => err,
        },
        Err(err) => Error::from(err),
    };
    TokenStream::from(err.write_errors())
}
