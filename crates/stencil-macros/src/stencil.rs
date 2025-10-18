use darling::FromMeta;
use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use std::fmt::Debug;
use syn::parse::ParseStream;
use syn::spanned::Spanned;
use syn::{Error, FnArg, ItemFn, Pat, ReturnType, Signature, Type};

pub struct StencilFn {
    name: Ident,
    sig: StencilSignature,
    orig: ItemFn,
}
impl Debug for StencilFn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StencilFn")
            .field("name", &self.name)
            .field("sig", &self.sig)
            .finish()
    }
}

impl syn::parse::Parse for StencilFn {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let item: syn::Item = input.parse()?;
        match item {
            syn::Item::Fn(mut orig) => {
                let sig = StencilSignature::parse(&mut orig.sig)?;
                Ok(StencilFn {
                    name: orig.sig.ident.clone(),
                    sig,
                    orig,
                })
            },
            _ => Err(Error::new_spanned(item, "use #[stencil] on functions"))?
        }
    }
}

#[derive(Default, FromMeta)]
#[darling(default)]
pub struct StencilMeta {
    #[darling(rename = "return")]
    no_tail_call: bool,
}

impl StencilFn {
    pub fn expand_inner(self, meta: StencilMeta) -> TokenStream {
        let StencilFn { name, orig, sig } = self;
        let StencilSignature {
            holes,
            input,
            output, assertions,
        } = sig;

        let output = generate_named("in", output);
        let output_ret: Vec<_> = output.iter()
            .map(|(_, ty)| quote!(#ty)).collect();
        let output_sig: Vec<_> = output.iter()
            .map(|(ident, ty)| quote!(#ident: #ty)).collect();

        let mut pass_args_to_impl: Vec<_> = output.iter()
            .take(input.len())
            .map(|(id, _)| quote!(#id))
            .collect();
        pass_args_to_impl.truncate(input.len());
        for (hole, _) in &holes {
            pass_args_to_impl.push(hole.to_token_stream());
        }
        let pass_args_to_output: Vec<_> = output.iter().map(|(id, _)| id).collect();

        let (extern_decls, init_holes) = generate_holes(holes);

        let (tail_call, tail_ret) = if meta.no_tail_call {
            (quote!(return #(#pass_args_to_output),*;), quote!((#(#output_ret),*)))
        } else {
            (quote!(become imp::copy_and_patch_next(#(#pass_args_to_output),*);), quote!(()))
        };

        let abi = "C";

        quote! {
            #[unsafe(no_mangle)]
            pub unsafe extern #abi fn #name(#(#output_sig),*) -> #tail_ret {
                mod imp {
                    #(#assertions)*

                    #[inline]
                    #orig

                    #[inline]
                    #[allow(unused)]
                    #[allow(useless_transmute)]
                    pub unsafe fn arg_pass(#(#output_sig),*) -> (#(#output_ret),*) {
                        unsafe {
                            #init_holes
                            #name(#(#pass_args_to_impl),*)
                        }
                    }

                    unsafe extern #abi {
                        #extern_decls
                        pub fn copy_and_patch_next(#(#output_sig),*);
                    }
                }
                unsafe {
                    let (#(#pass_args_to_output),*) = imp::arg_pass(#(#pass_args_to_output),*);
                    #tail_call
                }
            }
        }
    }

    pub fn expand(self, meta: StencilMeta) -> proc_macro::TokenStream {
        self.expand_inner(meta).into()
    }
}

fn generate_named(prefix: &str, types: Vec<Type>) -> Vec<(Ident, Type)> {
    types.iter().cloned().enumerate()
        .map(|(i, ty)| {
            let ident = Ident::new(&format!("{}{}", prefix, i), ty.span());
            (ident, ty)
        }).collect()
}

fn generate_holes(holes: Vec<(Ident, Type)>) -> (TokenStream, TokenStream) {
    let static_idents: Vec<_> = holes.iter().enumerate()
        .map(|(i, (ident, _))| Ident::new(&format!("HOLE{}", i), ident.span()))
        .collect();
    let decls: Vec<_> = static_idents.iter().map(|ident| {
        quote!{
            static #ident: [u8; 65536];
        }
    }).collect();
    let init: Vec<_> = holes.iter().zip(static_idents)
        .map(|((ident, ty), static_i)| {
            quote! {
                let #ident: #ty = std::mem::transmute(#static_i.as_ptr() as usize);
            }
        }).collect();
    (quote!(#(#decls)*), quote!(#(#init)*))
}

struct StencilSignature {
    holes: Vec<(Ident, Type)>,
    input: Vec<(Ident, Type)>,
    output: Vec<Type>,
    assertions: Vec<TokenStream>,
}
impl Debug for StencilSignature {

    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StencilSignature")
            .field("holes", &input_to_debug(&self.holes))
            .field("input", &input_to_debug(&self.input))
            .field("output", &self.output.iter().map(|ty| ty.to_token_stream()).collect::<Vec<_>>())
            .finish()
    }
}
impl StencilSignature {
    fn parse(sig: &mut Signature) -> syn::Result<Self> {
        // args: input + holes
        let mut input = vec![];
        let mut holes = vec![];
        for arg in &mut sig.inputs {
            if let FnArg::Typed(typed) = arg {
                match typed.pat.as_ref() {
                    Pat::Ident(i) => {
                        let attrs: Vec<_> = typed.attrs.iter().filter(|&attr| !attr.path().is_ident("hole")).cloned().collect();
                        let is_input = attrs.len() == typed.attrs.len();
                        (if is_input {
                            if !holes.is_empty() {
                                return Err(Error::new_spanned(arg, "input args must come before holes"));
                            }
                            &mut input
                        } else {
                            &mut holes
                        }).push((i.ident.clone(), typed.ty.as_ref().clone()));
                        typed.attrs = attrs;
                    }
                    _ => {
                        return Err(Error::new_spanned(arg, "unsupported arg pattern"));
                    }
                }
            } else {
                return Err(Error::new_spanned(arg, "unsupported arg type"));
            }
        }

        // return value: output
        let ReturnType::Type(_, ret) = &sig.output else {
            return Err(Error::new_spanned(&sig.output, "return type expected"));
        };
        let output = Self::parse_return_value(ret);

        // abi type assertions: input/output should fit inside usize
        let mut abi_types: Vec<Type> = input.iter().map(|(_, ty)| ty.clone()).collect();
        abi_types.extend(holes.iter().map(|(_, ty)| ty.clone()));
        abi_types.extend_from_slice(&output);
        // compile time assertions
        let assertions = abi_types.iter().map(|ty| quote! {
            const _: [(); std::mem::size_of::<usize>() - std::mem::size_of::<#ty>()] = [];
        }).collect();
        Ok(Self { input, holes, output, assertions })
    }

    fn parse_return_value(ty: &Type) -> Vec<Type> {
        if let Type::Tuple(tuple) = ty {
            tuple.elems.iter().cloned().collect()
        } else {
            vec![ty.clone()]
        }
    }
}

fn input_to_debug(slice: &[(Ident, Type)]) -> Vec<String> {
    slice.iter().map(
        |(i, ty)| i.to_string() + ": " + &ty.to_token_stream().to_string(),
    ).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let stream: TokenStream = quote! {
            pub fn foo(a: usize, #[hole] b: usize) -> (usize, usize){
                (a, b)
            }
        };
        let stencil: StencilFn = syn::parse2(stream).unwrap();
        println!("{:?}", stencil.expand_inner(Default::default()).to_string());
    }
}
