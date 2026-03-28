//! Generates a family of stencils
//!
//! ## Internals
//!
//! ### Signatures
//!
//! There are multiple kinds of signatures for stencils:
//! - [FnSignature]: the user-provided function signature,
//!   including expected arguments and explicit holes;
//! - [StencilSignature]: signature of a single stencils,
//!   including register allocation and implicit holes
//!   for stack allocated arguments/return values.

use std::{num::NonZero, ops::Index};

use darling::{FromMeta, ast::NestedMeta};
use itertools::Itertools;
use once_cell::unsync::OnceCell;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use smallvec::SmallVec;
use syn::{ItemFn, spanned::Spanned};

use crate::perm::RegPermutation;

#[derive(Debug, Default, FromMeta)]
#[darling(default)]
struct FamilyOptions {
    returns: bool,
    abi: Option<syn::LitStr>,
    #[darling(rename = "n")]
    registers: u16,
}
impl FamilyOptions {
    const DEFAULT_REGISTERS: NonZero<u16> = NonZero::new(10).unwrap();

    fn registers(&self) -> NonZero<u16> {
        NonZero::new(self.registers).unwrap_or(Self::DEFAULT_REGISTERS)
    }

    fn abi(&mut self) -> &syn::LitStr {
        self.abi.get_or_insert_with(
            || syn::LitStr::new("rust-preserve-none", Span::call_site()),
        )
    }
}

#[derive(Debug)]
pub struct StencilFamily {
    name: String,
    options: FamilyOptions,
    sig: FnSignature,
    orig: ItemFn,
}
impl syn::parse::Parse for StencilFamily {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut f: syn::ItemFn = input.parse()?;
        let name = f.sig.ident.to_string();
        if name.contains("__") {
            return Err(syn::Error::new_spanned(f.sig.ident, "please use a name without __"));
        }
        let sig = FnSignature::parse(&mut f.sig)?;
        Ok(StencilFamily {
            name,
            options: Default::default(),
            sig,
            orig: f,
        })
    }
}

#[derive(Debug)]
enum CallArg {
    Stack,
    Arg(u16),
    Hole(u16),
}
#[derive(Debug)]
struct FnSignature {
    call_args: Vec<CallArg>,
    inputs: u16,
    outputs: u16,
    hole_names: Vec<syn::Ident>,
}
impl FnSignature {
    fn parse(sig: &mut syn::Signature) -> syn::Result<Self> {
        let mut call_args = vec![];
        let mut inputs = 0;
        let mut hole_names = vec![];
        for ty in sig.inputs.iter_mut() {
            let syn::FnArg::Typed(ty) = ty else {
                return syn::Result::Err(syn::Error::new_spanned(ty, "expected type"));
            };

            enum Kind { Hole, Arg, Stack }
            let mut kind = Kind::Arg;
            for (attr_i, attr) in ty.attrs.iter().enumerate() {
                let path = attr.path();
                if path.is_ident("hole") {
                    ty.attrs.remove(attr_i);
                    kind = Kind::Hole;
                    break;
                }
                if path.is_ident("stack") {
                    ty.attrs.remove(attr_i);
                    kind = Kind::Stack;
                    break;
                }
            }

            call_args.push(match kind {
                Kind::Hole => {
                    let hole_i = hole_names.len();
                    hole_names.push(syn::Ident::new(&format!("hole{}", hole_i), Span::call_site()));
                    CallArg::Hole(hole_i as u16)
                }
                Kind::Arg => {
                    let i = inputs;
                    inputs += 1;
                    CallArg::Arg(i as u16)
                }
                Kind::Stack => CallArg::Stack,
            });
        }
        let outputs = match &sig.output {
            syn::ReturnType::Default => 0,
            syn::ReturnType::Type(_rarrow, ty) => match ty.as_ref() {
                syn::Type::Tuple(type_tuple) => type_tuple.elems.len(),
                syn::Type::Path(_path) => 1,
                _ => todo!("unsupported return type: {:?}", ty),
            },
        };
        if inputs + hole_names.len() + outputs + 1 > u16::MAX as usize {
            return Err(syn::Error::new_spanned(sig, "too many arguments"));
        }
        Ok(FnSignature {
            call_args,
            inputs: inputs as u16,
            outputs: outputs as u16,
            hole_names,
        })
    }
}

impl StencilFamily {
    pub fn set_options(&mut self, attr: TokenStream) -> syn::Result<()> {
        let attr = NestedMeta::parse_meta_list(attr)?;
        self.options = FamilyOptions::from_list(&attr)?;
        Ok(())
    }

    pub fn expand(mut self) -> TokenStream {
        let perm = RegPermutation::new(self.sig.inputs, self.sig.outputs, self.options.registers());
        let orig = self.orig.clone();
        let stencils = perm.map(|perm| self.generate_stencil(&perm));

        quote! {
            #[inline]
            #orig

            #(#stencils)*
        }
    }

    fn generate_stencil(
        &mut self, input_locations: &[u16],
    ) -> TokenStream {
        let name = syn::Ident::new(
            &format!("{}__{}", self.name, input_locations.iter().join("_")),
            self.name.span(),
        );

        let sig = StencilSignature::new(self.sig.inputs as usize, input_locations);
        let arg_list = &sig.arg_list();
        let [hole_defs, hole_inits, hole_outputs] = self.generate_holes(&sig);
        let call = self.generate_call(&sig);
        let [return_type, next_def, next_call] = self.generate_next(arg_list, &sig);

        let abi = self.options.abi();
        quote! {
            #[unsafe(no_mangle)]
            pub unsafe extern #abi fn #name(stack: &mut Stack #arg_list) -> #return_type {
                mod imp {
                    unsafe extern #abi {
                        #(#hole_defs)*
                        #next_def
                    }
                }

                #(#hole_inits)*
                #call
                #(#hole_outputs)*
                #next_call
            }
        }
    }

    fn generate_call(&self, sig: &StencilSignature) -> TokenStream {
        let name = &self.orig.sig.ident;
        let stack = OnceCell::new();
        let call_args = &self.sig.call_args;
        let mut args = Vec::with_capacity(call_args.len());
        for arg in call_args {
            args.push(match arg {
                CallArg::Arg(i) => {
                    match sig.io_locations[*i as usize] {
                        WrapperCallArg::Reg(i) => {
                            &sig.reg_names[i as usize]
                        }
                        WrapperCallArg::Stack(i) => {
                            &sig.stack_arg_names[i as usize]
                        },
                    }
                }
                CallArg::Hole(i) => {
                    &self.sig.hole_names[*i as usize]
                }
                CallArg::Stack => {
                    stack.get_or_init(|| syn::Ident::new("stack", Span::call_site()))
                }
            });
        }
        let rets = sig.io_locations[sig.inputs_num..].iter().map(|i| match i {
            WrapperCallArg::Reg(i) => {
                &sig.reg_names[*i as usize]
            }
            WrapperCallArg::Stack(i) => {
                &sig.stack_arg_names[*i as usize]
            },
        });
        let rets = if rets.len() == 1 {
            quote!(#(#rets),*)
        } else {
            quote!{ (#(#rets),*) }
        };
        quote! {
            let #rets = #name(#(#args.into()),*);
        }
    }

    fn generate_holes(&self, sig: &StencilSignature) -> [Vec<TokenStream>; 3] {
        let total = self.sig.hole_names.len() + sig.stack_arg_names.len();
        let mut hole_defs = Vec::with_capacity(total);
        let mut hole_inits = vec![];
        let mut hole_outputs = vec![];

        for hole_var in &self.sig.hole_names {
            let hole_sym = syn::Ident::new(&format!("{}__{}", self.name, hole_var), Span::call_site());
            hole_inits.push(quote! {
                let #hole_var = imp::#hole_sym.as_ptr() as usize;
            });
            hole_defs.push(hole_sym);
        }
        for hole in &sig.io_locations[..sig.inputs_num] {
            if let WrapperCallArg::Stack(i) = hole {
                let hole_var = &sig.stack_arg_names[*i as usize];
                let hole_sym = syn::Ident::new(&format!("{}__{}", self.name, hole_var), Span::call_site());
                hole_inits.push(quote! {
                    let #hole_var = stack.get(imp::#hole_sym.as_ptr() as usize);
                });
                hole_defs.push(hole_sym);
            }
        }
        for hole in &sig.io_locations[sig.inputs_num..] {
            if let WrapperCallArg::Stack(i) = hole {
                let hole_var = &sig.stack_arg_names[*i as usize];
                let hole_sym = syn::Ident::new(&format!("{}__{}", self.name, hole_var), Span::call_site());
                hole_outputs.push(quote! {
                    stack.set(imp::#hole_sym.as_ptr() as usize, #hole_var.into());
                });
                hole_defs.push(hole_sym);
            }
        }
        let hole_defs = hole_defs.iter().map(|sym| quote! {
            pub static #sym: [u8; 0x10000];
        }).collect();

        [hole_defs, hole_inits, hole_outputs]
    }

    fn generate_next(&self, arg_list: &TokenStream,sig: &StencilSignature) -> [TokenStream; 3] {
        let next_call_args = &sig.reg_names;
        if self.options.returns {
            [sig.return_type(), quote! {}, quote! {
                (#(#next_call_args.into()),*)
            }]
        } else {
            [quote! {()}, quote! {
                pub fn copy_and_patch_next(stack: &mut super::Stack #arg_list);
            }, quote! {
                become imp::copy_and_patch_next(stack #(, #next_call_args.into())*);
            }]
        }
    }
}

#[derive(Debug)]
struct StencilSignature {
    inputs_num: usize,
    reg_names: Vec<syn::Ident>,
    stack_arg_names: Vec<syn::Ident>,
    io_locations: Vec<WrapperCallArg>,
}
#[derive(Debug)]
enum WrapperCallArg {
    Reg(u16),
    Stack(u16),
}
impl StencilSignature {
    fn new(inputs_num: usize, io_locations: &[u16]) -> Self {
        let max_regs = io_locations.iter().cloned().max().unwrap_or(0);

        let mut stack_arg_names = vec![];
        let io_locations: Vec<_> = io_locations.iter().map(|i| {
            if *i == 0 {
                let index = stack_arg_names.len();
                stack_arg_names.push(syn::Ident::new(&format!("stack{}", index), Span::call_site()));
                WrapperCallArg::Stack(index as u16)
            } else {
                WrapperCallArg::Reg(*i - 1)
            }
        }).collect();

        let in_regs = Regs::from_args(&io_locations[..inputs_num], max_regs);
        let out_regs = Regs::from_args(&io_locations[inputs_num..], max_regs);

        let reg_names: Vec<_> = (0..max_regs).map(|i| syn::Ident::new(&match (&in_regs[i], &out_regs[i]) {
            (Register::Pass(i), Register::Pass(_)) => format!("pass{}", i),
            (Register::Var(i), Register::Pass(_)) => format!("in{}", i),
            (Register::Pass(_), Register::Var(j)) => format!("out{}", j),
            (Register::Var(i), Register::Var(j)) => format!("in{}_out{}", i, j),
        }, Span::call_site())).collect();

        StencilSignature {
            inputs_num,
            reg_names,
            stack_arg_names,
            io_locations,
        }
    }

    fn arg_list(&self) -> TokenStream {
        let args = self.reg_names.iter().map(
            |name| quote! { , #name: usize }
        );
        quote! { #(#args)* }
    }

    fn return_type(&self) -> TokenStream {
        if self.reg_names.len() == 1 {
            quote! { usize }
        } else {
            let usizes = std::iter::repeat_n(quote! { usize }, self.reg_names.len());
            quote! { (#(#usizes),*) }
        }
    }
}

/// Describes what a register is used for
#[derive(Clone, Copy, Debug)]
enum Register {
    Pass(u16),
    Var(u16),
}
#[derive(Debug)]
/// What each register argument in copy-and-patch is used for
struct Regs(SmallVec<[Register; 8]>);
impl Index<u16> for Regs {
    type Output = Register;
    fn index(&self, index: u16) -> &Self::Output {
        &self.0[index as usize]
    }
}
impl Regs {
    fn all_pass(len: u16) -> Self {
        let mut regs = SmallVec::from_elem(Register::Pass(0), len as usize);
        for i in 0..len {
            regs[i as usize] = Register::Pass(i + 1);
        }
        Self(regs)
    }

    fn from_args(perm: &[WrapperCallArg], regs: u16) -> Self {
        let mut regs = Self::all_pass(regs);
        for (i, reg) in perm.iter().enumerate() {
            if let WrapperCallArg::Reg(reg) = reg {
                regs.0[*reg as usize] = Register::Var(i as u16);
            }
        }
        regs
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_simple() {
        let stream = quote! {
            pub fn add(#[stack] stack: &mut Stack, #[hole] n: usize) {
                stack.allocate(n);
            }
        };

        let mut stencil: StencilFamily = syn::parse2(stream).unwrap();
        stencil.options.registers = 2;
        println!("{}", stencil.expand());
    }
}
