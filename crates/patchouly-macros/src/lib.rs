mod perm;
mod stencil;

use proc_macro::TokenStream;
use syn::parse_macro_input;

use crate::stencil::StencilFamily;

#[proc_macro_attribute]
pub fn stencil(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut function = parse_macro_input!(item as StencilFamily);
    if let Err(err) = function.set_options(attr.into()) {
        return err.into_compile_error().into();
    }
    function.expand().into()
}
