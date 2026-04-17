use proc_macro2::TokenStream;
use quote::quote;
use syn::{LitByteStr, LitStr, MetaNameValue, punctuated::Punctuated, token::Comma};

pub fn setup(args: Punctuated<MetaNameValue, Comma>) -> TokenStream {
    let Some(name) = args.iter().find(|m| m.path.is_ident("name")) else {
        return syn::Error::new_spanned(args, "expected name").to_compile_error();
    };
    let Some(name) = get_lit_str(&name.value) else {
        return syn::Error::new_spanned(name, "expected string literal").to_compile_error();
    };
    let v = name.value();
    let bytes = v.as_bytes();
    let len = bytes.len();
    let lit = LitByteStr::new(bytes, name.span());

    let mut extra_args = vec![];
    if let Some(registers) = args.iter().find(|m| m.path.is_ident("n")) {
        let v = &registers.value;
        extra_args.push(quote!(n = #v));
    }

    quote! {
        #[unsafe(no_mangle)]
        pub static __STENCIL_API_NAME: [u8; #len] = *#lit;

        const _: fn() = || {
            fn assert_impl_stack<T: ?Sized + ::patchouly_core::StencilStack>() {}
            assert_impl_stack::<Stack>();
        };

        /// An empty stencil used produce a no-op relative jump instruction
        #[::patchouly_macros::stencil(#(#extra_args,),*)]
        pub fn __empty() {
        }

        /// Stencils used to move values between registers/the stack
        #[::patchouly_macros::stencil(#(#extra_args,),*)]
        pub fn __move(v: usize) -> usize {
            v
        }

        /// A raw helper stencil used by the JIT to long-jump to runtime functions.
        #[::patchouly_macros::stencil(trampoline, abi = "Rust", #(#extra_args,),*)]
        pub fn __long_jump(#[hole] addr: usize) -> usize {
            addr
        }
    }
}

fn get_lit_str(expr: &syn::Expr) -> Option<LitStr> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(lit),
            ..
        }) => Some(lit.clone()),
        _ => None,
    }
}
