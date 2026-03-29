use proc_macro2::TokenStream;
use quote::quote;
use syn::{LitByteStr, LitStr};

pub fn setup(name: LitStr) -> TokenStream {
    let v = name.value();
    let bytes = v.as_bytes();
    let len = bytes.len();
    let lit = LitByteStr::new(bytes, name.span());
    quote! {
        #[unsafe(no_mangle)]
        pub static __STENCIL_API_NAME: [u8; #len] = *#lit;

        const _: fn() = || {
            fn assert_impl_stack<T: ?Sized + ::patchouly_core::StencilStack>() {}
            assert_impl_stack::<Stack>();
        };

        #[::patchouly_macros::stencil]
        pub fn __empty() {
        }
    }
}
